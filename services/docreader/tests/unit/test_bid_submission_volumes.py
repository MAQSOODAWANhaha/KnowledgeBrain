"""A source instruction to submit separate files must survive the default W2 path into each Word."""

from io import BytesIO
import json
from zipfile import ZipFile

from docx import Document
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.authoring_pipeline import generate_document
from docreader.tender.bid_format_outline import BID_FORMAT_OUTLINE_CONTRACT
from docreader.tender.item_placement import REQUIREMENT_CHAIN_CONTRACT
from test_phase1_bid_format import _snapshot


@pytest.mark.parametrize('instruction, separate', [
    ('商务文件、技术文件和报价文件须分别上传。', True),
    ('商务文件、技术文件和报价文件为三个部分。', False),
    ('商务文件、技术文件和报价文件不得分别上传。', False),
])
async def test_default_outline_exports_only_explicit_separate_submissions(tmp_path, instruction, separate):
    pages = [instruction + '\n3.1 投标文件的组成\n3.1.1 商务文件：应包括下列原式。\n'
             '(1) 投标函（附件1A）、\n授权书（附件1B）。\n'
             '3.1.2 技术文件：须响应技术要求。\n(1) 产品说明（附件4）。\n'
             '3.1.3 报价文件：按以下顺序编制。\n(1) 开标表（附件9）。\n(2) 分项表（附件2）。',
             '第六章 投标文件格式\n附件1A 投标函\n投标人签章：\n附件1B 授权书\n代表签字：\n'
             '附件2 分项表\n分项金额：________\n附件4 产品说明\n产品说明原式\n'
             '附件9 开标表\n总额：________\n附件11 专项声明\n其他原式']
    documents = [{'document_id': 'source', 'role': 'main', 'pages': pages}]

    async def choose(task, request, validate):
        assert task == 'directory_plan'
        lines = json.loads(request['messages'][1]['content'][0]['text'])['lines']
        return validate({'chapters': [{'heading_refs': [ref], 'name': name.split()[-1]}
                                      for ref, name in lines if name.startswith('附件')]})

    artifacts = ArtifactStore(tmp_path)
    snapshot = {**_snapshot(), 'frozen_ref': artifacts.put_json({'documents': []})}
    ref = await generate_document(artifacts, snapshot, RequestBudget(model='unused'), choose,
                                  directory_documents=documents, outline_only=True,
                                  directory_contract=BID_FORMAT_OUTLINE_CONTRACT,
                                  requirement_chain_contract=REQUIREMENT_CHAIN_CONTRACT)
    outline = artifacts.read_json(ref)
    result = export_authoring_document(artifacts, outline, [])
    with ZipFile(BytesIO(artifacts.read_bytes(result['package_ref']))) as package:
        volumes = json.loads(package.read('manifest.json'))['volumes']
        assert [v['title'] for v in volumes] == (['商务文件', '技术文件', '报价文件'] if separate else [])
        members = [name for name in package.namelist() if name.endswith('.docx')]
        assert len(members) == 1 + len(volumes)
        if separate:
            technical = next(c for c in outline['chapters'] if c['title'] == '技术文件')
            assert any(t['kind'] == 'narrative' and t['support_origins'] == ['knowledge']
                       and '须响应技术要求' in t['purpose'] for t in technical['targets'])
            bodies = ['\n'.join(p.text for p in Document(BytesIO(package.read(v['package_path']))).paragraphs)
                      for v in volumes]
            business, tech, price = bodies
            assert '投标人签章' in business and '代表签字' in business and '产品说明原式' in tech
            assert price.index('总额：') < price.index('分项金额：')
            assert all('分项金额：' not in text and '总额：' not in text for text in (business, tech))
            assert all('其他原式' not in text for text in bodies)
            assert any('专项声明' in i['detail'] and '分册' in i['detail'] for i in outline['issues'])


def test_outside_format_candidate_inside_a_volume_is_pruned_independently_of_its_siblings():
    from docreader.tender.item_placement import bind_placements, placement_positions
    from test_item_placement import item, outline

    tree = outline()
    children = tree['chapters']
    tree['chapters'] = [{'chapter_id': 'v', 'title': '商务文件', 'is_volume': True,
                         'targets': [], 'children': children}]
    positions = placement_positions(tree, {children[0]['title']: set()})
    assert positions['t1']['outside_format'] and not positions['t2']['outside_format']
    removed = bind_placements(tree, positions, {'r1': item(1)}, {'r1': None})
    assert removed['chapters'][0]['children'] == [children[1]]
    kept = bind_placements(tree, positions, {'r1': item(1)}, {'r1': 't1'})
    assert len(kept['chapters'][0]['children']) == 2


@pytest.mark.parametrize('instruction, expected', [
    ('投标人必须填写本表。', 2), ('如有额外服务，投标人须填写本表。', 0),
])
async def test_required_native_grids_in_the_cited_book_survive_a_source_only_draft(tmp_path, instruction, expected):
    from test_word_continuation_export import _grid, _store

    artifacts = ArtifactStore(tmp_path)
    pages = ['商务文件和技术文件分别提交。\n3.1.1 商务文件：\n(1) 投标函（附件1）。\n'
             '3.1.2 技术文件：须响应第二卷技术规范的要求。', '第二卷 技术规范',
             f'技术附表 G 差异应答表\n{instruction}\n附表 G.1 第一项\n字段甲\n附表 G.2 第二项\n字段乙',
             '技术附表 H 附加报告（本次不适用）\n投标人必须填写本表。\n附表 H.1 报告\n字段丙',
             '第三卷 投标文件', '第六章 投标文件格式\n附件1 投标函\n签章：']
    owners = [[], [], [None, None, None, 'a', None, 'b'], [None, None, None, 'c'], [], []]
    units = [{'unit_id': uid, 'kind': 'table_region', 'ordinal': i, 'text': '', 'grid': _grid(label),
              'locator': {'locator_kind': 'document', 'section_ordinal': 0}}
             for i, (uid, label) in enumerate(zip('abc', ('字段甲', '字段乙', '字段丙'), strict=True))]
    index = _store(artifacts, units)
    documents = [{'document_id': 'source', 'role': 'main', 'pages': pages, 'line_units': owners,
                  'template_parts': {u['unit_id']: {'source_ref': {'source_index_ref': index, 'unit_id': u['unit_id']},
                                                   'selectors': []} for u in units}}]

    async def choose(task, request, validate):
        lines = json.loads(request['messages'][1]['content'][0]['text'])['lines']
        ref = next(ref for ref, text in lines if text == '附件1 投标函')
        return validate({'chapters': [{'heading_refs': [ref], 'name': '投标函'}]})

    snapshot = {**_snapshot(), 'frozen_ref': artifacts.put_json({'documents': []})}
    ref = await generate_document(artifacts, snapshot, RequestBudget(model='unused'), choose,
                                  directory_documents=documents, outline_only=True,
                                  directory_contract=BID_FORMAT_OUTLINE_CONTRACT,
                                  requirement_chain_contract=REQUIREMENT_CHAIN_CONTRACT)
    result = export_authoring_document(artifacts, artifacts.read_json(ref), [])
    with ZipFile(BytesIO(artifacts.read_bytes(result['package_ref']))) as package:
        technical, = [v for v in json.loads(package.read('manifest.json'))['volumes'] if v['title'] == '技术文件']
        word = Document(BytesIO(package.read(technical['package_path'])))
        assert len(word.tables) == expected
        if expected:
            assert [table.cell(1, 0).text for table in word.tables] == ['字段甲', '字段乙']
            from docreader.tender.submission_book_forms import required_book_forms
            covered = [{'lines': [{'document_id': 'source', 'page': 2, 'line': 3}]}]
            missing, issues = required_book_forms(documents[0], {2: '技术文件'}, covered)
            assert [c['heading']['line'] for c in missing] == [4]
            assert not any(issue['code'] == 'outline_directory_table_unbound' for issue in issues)
