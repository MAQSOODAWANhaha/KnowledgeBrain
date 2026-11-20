"""Phase 1 copies the tender's bid-format section into the outline and the Word file."""

from io import BytesIO
from zipfile import ZipFile

from docx import Document
from lxml import etree

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.authoring_pipeline import generate_document
from docreader.tender.schema import AuthoringInput

W = '{http://schemas.openxmlformats.org/wordprocessingml/2006/main}'

PAGES = [
    '第一章 招标公告\n1. 标书费按照招标公告缴纳。\n未选的资格条款，不进入投标文件。\n',
    '第六章 投标文件格式\n一、投标函\n致：招标人\n1. 我方已仔细研究了招标文件。\n投标人名称：________\n',
    '二、报价表\n列甲\t列乙\n单价\t\n',
]


def _documents():
    return [{
        'document_id': 'tender', 'role': 'main', 'pages': PAGES,
        'line_units': [[] for _ in PAGES],
        'unread_pages': [2],
        'ocr_issues': [{'code': 'outline_directory_page_unreadable', 'detail': '第3页未能读取，报价表保留待核'}],
    }]


def _snapshot():
    return AuthoringInput.model_validate({
        'schema_version': 2, 'run_id': 'phase1', 'source_version': 1, 'domain_revision': 1,
    }).model_dump()


async def _outline(tmp_path, knowledge=None, documents=None, artifacts=None, execute=None, directory_contract=None):
    artifacts = artifacts or ArtifactStore(tmp_path)
    ref = await generate_document(
        artifacts, _snapshot(), RequestBudget(model='unused'), execute,
        directory_documents=documents or _documents(), outline_only=True, knowledge=knowledge,
        directory_contract=directory_contract)
    return artifacts, artifacts.read_json(ref)


def _purposes(outline):
    found = []
    def walk(chapters):
        for chapter in chapters:
            for target in chapter.get('targets') or []:
                if target.get('purpose'):
                    found.append(target['purpose'])
            walk(chapter.get('children') or [])
    walk(outline['chapters'])
    return found


VOLUMES = [
    '3.1 投标文件的组成\n'
    '投标文件应包括商务文件（不含报价文件）、技术文件、报价文件。\n'
    '3.1.1 商务文件：投标人提交的商务投标文件。\n'
    '(1) 投标函(附件 1A)；\n'
    '3.1.2 技术文件：须响应技术规范。\n'
    '3.1.3 报价文件：\n'
    '(1) 报价表(附件 2)。\n',
    '第六章 投标文件格式\n投标函的格式(1A)\n致：招标人\n二、货物规格一览表\n规格正文。\n附件 2 报价表\n单价\n',
]


def _children(outline):
    return [(chapter['title'], [child['title'] for child in chapter.get('children') or []])
            for chapter in outline['chapters']]


def _flat(chapters):
    found = []
    for chapter in chapters:
        found.append(chapter['title'])
        found.extend(_flat(chapter.get('children') or []))
    return found


def _forms(outline):
    return [title for title in _flat(outline['chapters']) if title not in ('商务文件', '技术文件', '报价文件')]


async def test_phase1_splits_business_technical_and_price_volumes(tmp_path):
    documents = [{
        'document_id': 'tender', 'role': 'main', 'pages': VOLUMES,
        'line_units': [[] for _ in VOLUMES], 'unread_pages': [], 'ocr_issues': [],
    }]
    _artifacts, outline = await _outline(tmp_path, documents=documents)
    assert _children(outline) == [
        ('商务文件', ['投标函']),
        ('技术文件', ['货物规格一览表']),
        ('报价文件', ['报价表']),
    ]
    purposes = _purposes(outline)
    assert any('致：招标人' in text for text in purposes)
    assert any('规格正文。' in text for text in purposes)
    assert any('单价' in text for text in purposes)


async def test_phase1_technical_requirement_headings_become_chapters(tmp_path):
    pages = [
        '3.1.2 技术文件：须响应技术规范。\n',
        '7、售后服务\n质保期内接到通知后 12 小时到场。\n8、预装调试要求\n完成预装和调试。\n',
        '第六章 投标文件格式\n一、投标函\n致：招标人\n',
    ]
    documents = [{
        'document_id': 'tender', 'role': 'main', 'pages': pages,
        'line_units': [[] for _ in pages], 'unread_pages': [], 'ocr_issues': [],
    }]
    _artifacts, outline = await _outline(tmp_path, documents=documents)
    technical = next(chapter for chapter in outline['chapters'] if chapter['title'] == '技术文件')
    titles = [child['title'] for child in technical['children']]
    assert '售后服务' in titles and '预装调试要求' in titles
    purposes = _purposes(outline)
    assert any('12 小时到场' in text for text in purposes)
    assert any('致：招标人' in text for text in purposes)


async def test_phase1_outline_copies_only_the_bid_format(tmp_path):
    _artifacts, outline = await _outline(tmp_path)
    titles = _flat(outline['chapters'])
    assert [title for title in titles if title not in ('商务文件', '技术文件', '报价文件')] == ['投标函', '报价表']
    assert not any('标书费' in title or '我方' in title for title in titles)
    purposes = _purposes(outline)
    assert any('致：招标人' in text and '1. 我方已仔细研究了招标文件。' in text
               and '投标人名称：________' in text for text in purposes)
    assert any('列甲' in text and '列乙' in text for text in purposes)
    joined = '\n'.join(purposes)
    assert '未选的资格条款' not in joined
    assert '标书费按照招标公告' not in joined
    assert '我方承诺满足' not in joined
    assert any(issue['code'] == 'outline_directory_page_unreadable' for issue in outline['issues'])
    assert any('报价表' in text for text in purposes)


async def test_phase1_knowledge_hit_does_not_fill_the_template(tmp_path):
    class FakeKnowledge:
        def spec(self):
            return {'provider': 'fake'}

        async def search(self, _search, _artifacts):
            return {'materials': [{
                'material_id': 'hit', 'title': '知识', 'text': '我方已具备全部资格并报价100万元',
                'content_sha256': 'a' * 64, 'locator': {'document_id': 'other', 'source_sha256': 'b' * 64},
            }], 'issues': []}

    _artifacts, outline = await _outline(tmp_path, knowledge=FakeKnowledge())
    blob = '\n'.join(_purposes(outline))
    assert '我方已具备全部资格并报价100万元' not in blob
    assert '致：招标人' in blob
    for chapter in outline['chapters']:
        for target in chapter.get('targets') or []:
            if target.get('support_origins') == ['knowledge']:
                assert not (target.get('purpose') or '').strip() or target['purpose'] == target.get('related_target_ids')


async def test_phase1_export_writes_tender_templates_without_invented_widths(tmp_path):
    from test_word_continuation_export import _grid, _store

    artifacts = ArtifactStore(tmp_path)
    unit = {'unit_id': 'quote', 'kind': 'table_region', 'ordinal': 0, 'text': '',
            'grid': _grid('单价'), 'locator': {'locator_kind': 'document', 'section_ordinal': 0}}
    ref = _store(artifacts, [unit])
    documents = _documents()
    documents[0]['line_units'] = [[], [], [None, 'quote', 'quote']]
    documents[0]['template_parts'] = {'quote': {
        'source_ref': {'source_index_ref': ref, 'unit_id': 'quote'}, 'selectors': []}}
    artifacts, outline = await _outline(tmp_path, documents=documents, artifacts=artifacts)
    result = export_authoring_document(artifacts, outline, [])
    package = artifacts.read_bytes(result['docx_ref'])
    word = Document(BytesIO(package))
    paragraphs = [paragraph.text for paragraph in word.paragraphs]
    cells = [cell.text for table in word.tables for row in table.rows for cell in row.cells]
    assert any('致：招标人' in text for text in paragraphs)
    assert any('列甲' in text for text in [*paragraphs, *cells])
    assert not any('我方承诺' in text for text in paragraphs)
    xml = etree.fromstring(ZipFile(BytesIO(package)).read('word/document.xml'))
    caches = [text.text for field in xml.findall('.//' + W + 'fldSimple')
              if 'PAGEREF' in (field.get(W + 'instr') or '')
              for text in field.findall('.//' + W + 't')]
    assert caches and all(cache == '待计算' for cache in caches)
    tables = xml.findall('.//' + W + 'tbl')
    assert tables
    for table in tables:
        assert table.find('.//' + W + 'tcW') is None
        columns = table.findall('.//' + W + 'gridCol')
        assert columns and all(column.get(W + 'w') is None for column in columns)


MAPPED = [
    '第六章 投标文件格式\n附件 1 投标函、法定代表人身份证明\n投标函的格式(1A)\n致：招标人\n1. 我方已仔细研究了招标文件。\n',
    '附件 8B\n投标单位股权结构说明及相关证明文件\n',
]


def _mapped_documents():
    return [{
        'document_id': 'tender', 'role': 'main', 'pages': MAPPED,
        'line_units': [[] for _ in MAPPED], 'unread_pages': [], 'ocr_issues': [],
    }]


async def test_phase1_does_not_turn_an_annex_mapping_into_chapter_titles(tmp_path):
    _artifacts, outline = await _outline(tmp_path, documents=_mapped_documents())
    titles = _forms(outline)
    assert titles == ['投标函', '投标单位股权结构说明及相关证明文件']
    assert not any(title.startswith('附件') for title in titles)
    blob = '\n'.join(_purposes(outline))
    assert '致：招标人' in blob and '1. 我方已仔细研究了招标文件。' in blob
    assert '法定代表人身份证明' not in titles


async def test_phase1_model_selects_annex_file_names_from_tender_lines(tmp_path):
    import json

    from docreader.llm.request import ModelContractError

    def lines_of(request):
        body = json.loads(request['messages'][1]['content'][0]['text'])
        return body['lines']

    async def execute(task, request, validate):
        assert task == 'directory_plan'
        assert '映射' in request['messages'][0]['content']
        found = {text.strip(): ref for ref, text in lines_of(request)}
        raw = {'chapters': [
            {'heading_refs': [found['投标函的格式(1A)']], 'name': '投标函',
             'body': [found['投标函的格式(1A)'], found['1. 我方已仔细研究了招标文件。']]},
            {'heading_refs': [found['附件 1 投标函、法定代表人身份证明']], 'name': '法定代表人身份证明',
             'body': [found['附件 1 投标函、法定代表人身份证明'], found['附件 1 投标函、法定代表人身份证明']]},
            {'heading_refs': [found['投标单位股权结构说明及相关证明文件']],
             'name': '投标单位股权结构说明及相关证明文件',
             'body': [found['附件 8B'], found['投标单位股权结构说明及相关证明文件']]},
        ]}
        return validate(raw)

    _artifacts, outline = await _outline(
        tmp_path, documents=_mapped_documents(), execute=execute,
        directory_contract='bid_format_source_outline_v1')
    titles = [chapter['title'] for chapter in outline['chapters']]
    assert titles == ['法定代表人身份证明', '投标函', '投标单位股权结构说明及相关证明文件']
    assert not any(title.startswith('附件') for title in titles)
    purposes = _purposes(outline)
    assert any('致：招标人' in text and '1. 我方已仔细研究了招标文件。' in text for text in purposes)
    assert any('投标单位股权结构说明及相关证明文件' in text for text in purposes)
    assert not any('我方承诺' in text for text in purposes)

    async def invented(_task, request, validate):
        found = {text.strip(): ref for ref, text in lines_of(request)}
        ref = found['投标函的格式(1A)']
        return validate({'chapters': [{'heading_refs': [ref], 'name': '我方承诺中标', 'body': [ref, ref]}]})

    try:
        await _outline(tmp_path, documents=_mapped_documents(), execute=invented,
                       directory_contract='bid_format_source_outline_v1')
    except ModelContractError as error:
        assert any('文件名' in item['detail'] for item in error.errors)
    else:
        raise AssertionError('an invented chapter name must be rejected')


FORMAT_NAMES = [
    '第六章 投标文件格式\n'
    '附件 1 投标函、法定代表人身份证明\n'
    '一、投标函\n'
    '二、法定代表人（单位负责人）身份证明（适用于无委托代理人的情况）；\n'
    '三、联合体协议书（如有）\n'
    '四、投标保证金\n'
    '五、资格证明文件\n'
    '六、投标分项报价表（投标人可根据自身实际情况填写）\n'
    '七、技术性能指标的详细描述\n'
    '八、投标函附录\n'
    '九、投标人根据评标办法需要提供的其他商务文件\n投标人自行编写。\n',
    '一、投标函\n致：招标人\n',
    '二、不向招标单位和招标代理机构项目相关人员套取影响招标活动公正性的项目关\n'
    '二、需要在原平台相关联的招标文件购\n买平台操作\n',
    '附件 8D （需提供投标人及原厂财务情况）\n8D-1 表 近年财务状况表\n一、注册资金万元\n二、净资产万元\n十二、主要财务指标\n',
    '附件 8B\n投标单位股权结构说明及相关证明文件(需明确出资人)\n',
    '附件 8E\n资信证明(AAA 级)或银行出具的资信证明材料。\n8E-1 表 企业资信等级证书情况\n',
    '附件 8F\n投标人提供原厂有效的资质等级证书（ISO9001 等）及相关证明文件。\n',
    '附件 8H\n或各级信用信息共享平台中列入失信被执行人名单(须提供截图)。\n',
    '附件 9 开标价格表及报价承诺函\n附件 9 开标价格表\n单价\n',
]


async def test_phase1_keeps_tender_file_names_and_drops_metric_rows(tmp_path):
    documents = [{
        'document_id': 'tender', 'role': 'main', 'pages': FORMAT_NAMES,
        'line_units': [[] for _ in FORMAT_NAMES], 'unread_pages': [], 'ocr_issues': [],
    }]
    _artifacts, outline = await _outline(tmp_path, documents=documents)
    titles = _forms(outline)
    for title in (
        '投标函', '法定代表人（单位负责人）身份证明（适用于无委托代理人的情况）', '联合体协议书（如有）',
        '投标保证金', '资格证明文件', '投标分项报价表（投标人可根据自身实际情况填写）',
        '技术性能指标的详细描述', '投标函附录', '投标人根据评标办法需要提供的其他商务文件',
        '近年财务状况表', '投标单位股权结构说明及相关证明文件', '企业资信等级证书情况',
        '资质等级证书', '失信被执行人名单', '开标价格表',
    ):
        assert title in titles
    price = next(chapter for chapter in outline['chapters'] if chapter['title'] == '报价文件')
    assert '投标分项报价表（投标人可根据自身实际情况填写）' in [child['title'] for child in price['children']]
    assert '开标价格表' in [child['title'] for child in price['children']]
    purposes = _purposes(outline)
    finance = next(text for text in purposes if '近年财务状况表' in text)
    assert '需提供投标人及原厂财务情况' in finance and '注册资金万元' in finance
    credit = next(text for text in purposes if '企业资信等级证书情况' in text)
    assert '资信证明(AAA 级)或银行出具的资信证明材料。' in credit
    assert '注册资金万元' not in credit
    assert '注册资金万元' not in titles
    assert '主要财务指标' not in titles
    assert not any('不向招标' in title or '招标文件购' in title for title in titles)
    assert titles.count('开标价格表') == 1
    assert titles.count('投标函') == 1
    assert any('致：招标人' in text for text in _purposes(outline))


QUOTED_CHAPTER = [
    '第六章 投标文件格式\n一、投标函\n二、法定代表人（单位负责人）身份证明\n三、授权委托书\n',
    '一、投标函\n致：招标人\n且不存在\n第二章“投标人须知”第 1.4.3 项规定的任何一种情形。\n投标人名称：________\n',
    '二、法定代表人（单位负责人）身份证明\n姓名：张三\n系投标人的法定代表人。\n',
    '三、授权委托书\n现委托李四为我方代理人。\n',
    '第七章 合同条款及格式\n合同正文不进入投标文件格式。\n',
]


async def test_phase1_a_quoted_chapter_does_not_swallow_the_later_form(tmp_path):
    documents = [{
        'document_id': 'tender', 'role': 'main', 'pages': QUOTED_CHAPTER,
        'line_units': [[] for _ in QUOTED_CHAPTER], 'unread_pages': [], 'ocr_issues': [],
    }]
    _artifacts, outline = await _outline(tmp_path, documents=documents)
    titles = _forms(outline)
    assert titles == ['投标函', '法定代表人（单位负责人）身份证明', '授权委托书']
    purposes = _purposes(outline)
    letter = next(text for text in purposes if '致：招标人' in text)
    identity = next(text for text in purposes if '姓名：张三' in text)
    warrant = next(text for text in purposes if '现委托李四为我方代理人。' in text)
    assert '第二章“投标人须知”第 1.4.3 项规定的任何一种情形。' in letter
    assert '投标人名称：________' in letter
    assert '姓名：张三' not in letter
    assert '现委托李四' not in letter
    assert '系投标人的法定代表人。' in identity
    assert '致：招标人' not in identity
    assert identity.strip() != '二、法定代表人（单位负责人）身份证明'
    assert '姓名：张三' not in warrant
    assert '合同正文不进入投标文件格式' not in '\n'.join(purposes)


SAME_ITEM = [
    '第六章 投标文件格式\n八、技术性能指标的详细描述\n九、技术支持资料\n',
    '八、投标产品技术性能指标的详细描述\n本页为性能指标模板。\n',
    '九、技术支持资料\n技术支持正文。\n',
]


async def test_phase1_a_longer_repeat_of_the_same_item_is_not_a_second_chapter(tmp_path):
    documents = [{
        'document_id': 'tender', 'role': 'main', 'pages': SAME_ITEM,
        'line_units': [[] for _ in SAME_ITEM], 'unread_pages': [], 'ocr_issues': [],
    }]
    _artifacts, outline = await _outline(tmp_path, documents=documents)
    titles = _forms(outline)
    assert titles == ['技术性能指标的详细描述', '技术支持资料']
    purposes = _purposes(outline)
    performance = next(text for text in purposes if '本页为性能指标模板。' in text)
    support = next(text for text in purposes if '技术支持正文。' in text)
    assert '八、投标产品技术性能指标的详细描述' in performance
    assert '技术支持正文' not in performance
    assert '本页为性能指标模板' not in support
