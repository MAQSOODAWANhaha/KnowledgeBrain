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
