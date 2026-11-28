"""Phase 1 copies the tender's bid-format section into the outline and the Word file."""

from io import BytesIO
from zipfile import ZipFile

from docx import Document
import lxml.etree as etree
import pytest

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
        '开标价格表',
    ):
        assert title in titles
    assert not {'资质等级证书', '失信被执行人名单'}.intersection(titles)
    body = '\n'.join(_purposes(outline))
    assert '投标人提供原厂有效的资质等级证书' in body and '列入失信被执行人名单' in body
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


SHARED_AND_WRAPPED = [
    '第六章 投标文件格式\n'
    '八、资格证明文件\n'
    '营业执照、资质证书（如有）等\n'
    '九、其他商务文件\n'
    '由省级以上监狱管理局、戒毒管理局(含新疆生产建设兵团)出具的属于监狱\n'
    '企业的证明文件\n'
    '证明文件正文。\n',
]


def _shared_and_wrapped_documents():
    return [{'document_id': 'tender', 'role': 'main', 'pages': SHARED_AND_WRAPPED,
             'line_units': [[] for _ in SHARED_AND_WRAPPED]}]


async def test_phase1_model_keeps_each_listed_file_and_each_wrapped_title(tmp_path):
    """BUG-301: one顿号 line may name several files. BUG-298: a wrapped title keeps every line."""
    import json

    async def execute(task, request, validate):
        body = json.loads(request['messages'][1]['content'][0]['text'])
        found = {text.strip(): ref for ref, text in body['lines']}
        listed = found['营业执照、资质证书（如有）等']
        first = found['由省级以上监狱管理局、戒毒管理局(含新疆生产建设兵团)出具的属于监狱']
        return validate({'chapters': [
            {'heading_refs': [found['八、资格证明文件']], 'name': '资格证明文件'},
            {'heading_refs': [listed], 'name': '营业执照'},
            {'heading_refs': [listed], 'name': '资质证书'},
            {'heading_refs': [first, first + 1],
             'name': '由省级以上监狱管理局、戒毒管理局(含新疆生产建设兵团)出具的属于监狱企业的证明文件'},
        ]})

    _artifacts, outline = await _outline(
        tmp_path, documents=_shared_and_wrapped_documents(), execute=execute,
        directory_contract='bid_format_source_outline_v1')
    titles = _flat(outline['chapters'])
    assert '营业执照' in titles and '资质证书' in titles
    assert '由省级以上监狱管理局、戒毒管理局(含新疆生产建设兵团)出具的属于监狱企业的证明文件' in titles
    assert any('营业执照、资质证书（如有）等' in text for text in _purposes(outline))


RUNNING = [
    '招标编号：A-1\n第六章 投标文件格式\n一、投标书\n致：招标人\n11\n',
    '招标编号：A-1\n我方承诺按招标文件提交。\n投标人名称：\n12\n',
    '招标编号：A-1\n二、开标一览表\n总价：\n13\n',
    '招标编号：A-1\n单位负责人签字：\n14\n',
]


async def test_phase1_model_copy_skips_running_headers_and_page_numbers(tmp_path):
    """BUG-302: a form copied across pages keeps its text, not the page edges."""
    import json

    from docreader.tender.directory_compile import _layout_decoration

    documents = [{'document_id': 'tender', 'role': 'main', 'pages': RUNNING,
                  'line_units': [[] for _ in RUNNING], 'layout_decoration': _layout_decoration(RUNNING)}]

    async def execute(task, request, validate):
        body = json.loads(request['messages'][1]['content'][0]['text'])
        found = {text.strip(): ref for ref, text in body['lines']}
        return validate({'chapters': [{'heading_refs': [found['一、投标书']], 'name': '投标书'},
                                      {'heading_refs': [found['二、开标一览表']], 'name': '开标一览表'}]})

    _artifacts, outline = await _outline(tmp_path, documents=documents, execute=execute,
                                         directory_contract='bid_format_source_outline_v1')
    text = '\n'.join(_purposes(outline))
    assert '我方承诺按招标文件提交。' in text and '单位负责人签字：' in text
    assert '招标编号' not in text
    assert not any(line.strip() in {'11', '12', '13', '14'} for line in text.splitlines())


@pytest.mark.parametrize('select_cover', [False, True])
async def test_original_cover_is_kept_before_toc_with_its_native_grid(tmp_path, select_cover):
    import json

    from test_word_continuation_export import _store

    artifacts = ArtifactStore(tmp_path)
    rows = [('招标编号：', '编号 A'), ('投标人名称：', '（盖章）'), ('日期：', '年 月 日')]
    grid = {'row_count': 3, 'column_count': 2, 'cells': [
        {'row': r, 'column': c, 'row_span': 1, 'col_span': 1, 'text': text}
        for r, row in enumerate(rows) for c, text in enumerate(row)
    ]}
    cover_table = {'unit_id': 'cover-grid', 'kind': 'table_region', 'ordinal': 0, 'text': '', 'grid': grid,
                   'locator': {'locator_kind': 'document', 'section_ordinal': 0}}
    ref = _store(artifacts, [cover_table])
    pages = [
        '第六章 投标文件格式\n21',
        '招标编号：编号 A\n采购项目固定名称\n投标文件\n招标编号：编号 A\n投标人名称：（盖章）\n日期：年 月 日\n22',
        '一、投标函\n原式固定正文\n23',
    ]
    documents = [{'document_id': 'tender', 'role': 'main', 'pages': pages,
                  'layout_decoration': [[], [True, False, False, True, False, False, False], []],
                  'line_units': [[], [None, None, None, 'cover-grid', 'cover-grid', 'cover-grid', None], []],
                  'template_parts': {'cover-grid': {'source_ref': {'source_index_ref': ref, 'unit_id': 'cover-grid'},
                                                    'selectors': []}}}]

    async def execute(_task, request, validate):
        payload = json.loads(request['messages'][1]['content'][0]['text'])
        lines = {text.strip(): ref for ref, text in payload['lines']}
        chapters = [{'heading_refs': [lines['一、投标函']], 'name': '投标函'}]
        if select_cover:
            chapters.insert(0, {'heading_refs': [lines['投标文件']], 'name': '投标文件'})
        return validate({'chapters': chapters})

    _, outline = await _outline(tmp_path, documents=documents, artifacts=artifacts, execute=execute,
                               directory_contract='bid_format_source_outline_v1')
    cover, letter = outline['chapters']
    assert (cover['title'], cover['presentation_role'], letter['title']) == ('投标文件', 'cover', '投标函')
    result = export_authoring_document(artifacts, outline, [])
    word = Document(BytesIO(artifacts.read_bytes(result['docx_ref'])))
    cover_grid, = word.tables
    assert [tuple(cell.text for cell in row.cells) for row in cover_grid.rows] == rows
    paragraphs = [p.text.strip() for p in word.paragraphs]
    assert paragraphs.index('采购项目固定名称') < paragraphs.index('目录')
    assert '招标编号：编号 A' not in paragraphs and '22' not in paragraphs


@pytest.mark.parametrize('signed', [True, False])
async def test_text_cover_uses_only_signature_fields_on_the_formats_own_page(tmp_path, signed):
    import json

    stamp = '投标人：________（盖单位章）'
    signature = '法定代表人或其委托代理人：________（签字）'
    date = '年    月    日'
    pages = [
        f'采购人自用封面\n投标文件\n{stamp}\n{signature}\n{date}',
        '第六章 投标文件格式',
        '招标文件页眉\n采购项目固定名称\n投 标 文 件\n' + ('\n'.join((stamp, signature, date)) if signed else ''),
        f'招标文件页眉\n一、投标函\n独立函件正文\n{stamp}\n{signature}\n{date}',
    ]
    docs = [{'document_id': 'tender', 'role': 'main', 'pages': pages}]

    async def select(_task, request, validate):
        payload = json.loads(request['messages'][1]['content'][0]['text'])
        ref = next(ref for ref, text in payload['lines'] if text == '一、投标函')
        return validate({'chapters': [{'heading_refs': [ref], 'name': '投标函'}]})

    artifacts, outline = await _outline(tmp_path, documents=docs, execute=select,
                                        directory_contract='bid_format_source_outline_v1')
    assert [c['presentation_role'] for c in outline['chapters']] == (['cover', 'body'] if signed else ['body'])
    exported = export_authoring_document(artifacts, outline, [])
    word = Document(BytesIO(artifacts.read_bytes(exported['docx_ref'])))
    paragraphs = [p.text.strip() for p in word.paragraphs]
    front = [text.replace('\xa0', ' ') for text in paragraphs[:paragraphs.index('目录')]]
    for field in ('采购项目固定名称', stamp, signature, date):
        assert (field in front) == signed
    assert '采购人自用封面' not in paragraphs and '招标文件页眉' not in front


def _not_applicable_formats():
    from docreader.tender.bid_format_outline import _Prepared

    pages = [
        '第六章 投标文件格式\n9-1 声明函\n正常表内正文\n说明：个别字段可注明（不适用）',
        '9-2 额外声明（不适用）\n1. 附随文件\n不适用式样固定正文',
        '9-3 业绩表\n须保留的另一表正文',
    ]
    prepared = _Prepared([{'document_id': 'tender', 'role': 'main', 'pages': pages}], _snapshot())
    assert len(prepared.sections) == 1
    refs = {text: ref for ref, text in prepared.sections[0]['payload']['lines']}
    return prepared, refs


@pytest.mark.parametrize('body_present', [False, True])
def test_inapplicable_catalog_annex_is_not_promoted_from_a_list_alone(body_present):
    from docreader.tender.bid_format_outline import _Prepared, decode_bid_format_outline

    pages = ['目录\n第六章 投标文件格式......30\n附件3 参数清单（本次不适用）......32',
             '第六章 投标文件格式', '目 录\n附件3 参数清单\n附件4 承诺函',
             '附件4 承诺函\n签字：_______']
    if body_present:
        pages.append('附件3 参数清单\n型号：_______')
    prepared = _Prepared([{'role': 'main', 'document_id': 'source', 'pages': pages}], {})
    ref = prepared.refs.by_point
    selected = [{'heading_refs': [ref['source', 2, 1]], 'name': '参数清单'},
                {'heading_refs': [ref['source', 3, 0]], 'name': '承诺函'}]
    if body_present:
        selected.append({'heading_refs': [ref['source', 4, 0]], 'name': '参数清单'})
    result = decode_bid_format_outline({'chapters': selected}, prepared)
    assert [node['heading']['page'] for node in result['nodes'] if node['name'] == '参数清单'] == (
        [4] if body_present else [])
    code = 'outline_directory_applicability_unconfirmed' if body_present else 'outline_directory_excluded'
    assert any(i['code'] == code and '参数清单' in i['detail'] and '第1页' in i['detail']
               for i in result['skipped_item_issues'])


@pytest.mark.parametrize('same_page', [True, False])
async def test_catalog_name_keeps_a_manual_slot_without_copying_adjacent_forms(tmp_path, same_page):
    import json
    from docreader.tender.acceptance_eval import _inspect_docx

    catalog = '目 录\n附件 1 报价表及价格承诺函\n附件 2 签署说明'
    forms = '附件 1 报价表\n单价：______\n附件 2 签署说明\n本说明应盖章。'
    pages = ['第六章 投标文件格式', *([catalog + '\n' + forms] if same_page else [catalog, forms])]
    docs = [{'document_id': 'source', 'role': 'main', 'pages': pages}]

    async def select(_task, request, validate):
        lines = json.loads(request['messages'][1]['content'][0]['text'])['lines']
        directory = next(ref for ref, line in lines if '及价格承诺函' in line)
        table = next(ref for ref, line in lines if line == '附件 1 报价表')
        signature = max(ref for ref, line in lines if line == '附件 2 签署说明')
        return validate({'chapters': [{'heading_refs': [directory], 'name': '价格承诺函'},
                                      {'heading_refs': [table], 'name': '报价表'},
                                      {'heading_refs': [signature], 'name': '签署说明'}]})

    artifacts, outline = await _outline(tmp_path, documents=docs, execute=select,
                                        directory_contract='bid_format_source_outline_v1')
    letter, table, signature = outline['chapters']
    target, = letter['targets']
    assert target['purpose'] == '价格承诺函' and target['manual_fill']
    assert any('价格承诺函' in issue['detail'] and '第 2 页' in issue['detail'] and '原式待核' in issue['detail']
               for issue in outline['issues'])
    exported = export_authoring_document(artifacts, outline, [])
    docx = _inspect_docx(artifacts.read_bytes(exported['docx_ref']), outline)
    assert docx['missing_target_ids'] == []
    text = '\n'.join(docx['texts_by_chapter'][letter['chapter_id']])
    assert '附件 2' not in text and '报价表' not in text
    assert '单价：______' in docx['texts_by_chapter'][table['chapter_id']]
    assert '本说明应盖章。' in docx['texts_by_chapter'][signature['chapter_id']]


def test_explicit_catalog_window_cannot_claim_another_entry():
    from docreader.llm.request import ModelContractError
    from docreader.tender.bid_format_outline import _Prepared, decode_bid_format_outline

    doc = {'document_id': 's', 'role': 'main', 'pages': ['第六章 投标文件格式\n目录\n附件1 承诺书\n附件2 说明函']}
    prepared = _Prepared([doc], {})
    catalog, other = [prepared.refs.by_point['s', 0, line] for line in (2, 3)]
    with pytest.raises(ModelContractError):
        decode_bid_format_outline({'chapters': [{'name': '承诺书', 'heading_refs': [catalog],
                                                'body': [catalog, other]}]}, prepared)


def test_skipped_format_still_bounds_an_implicit_copy_window():
    from docreader.tender.bid_format_outline import decode_bid_format_outline

    prepared, refs = _not_applicable_formats()
    result = decode_bid_format_outline({'chapters': [
        {'heading_refs': [refs['9-1 声明函']], 'name': '声明函'},
        {'heading_refs': [refs['9-3 业绩表']], 'name': '业绩表'},
    ]}, prepared)
    first, last = result['nodes']
    assert first['body_windows'] == [{'document_id': 'tender', 'page': 0, 'start_line': 1, 'end_line': 3}]
    assert last['body_windows'] == [{'document_id': 'tender', 'page': 2, 'start_line': 0, 'end_line': 1}]


@pytest.mark.parametrize('selection', ['crossing', 'excluded_heading', 'excluded_child'])
def test_selected_format_cannot_enter_a_marked_non_applicable_subtree(selection):
    from docreader.llm.request import ModelContractError
    from docreader.tender.bid_format_outline import decode_bid_format_outline

    prepared, refs = _not_applicable_formats()
    first = {'heading_refs': [refs['9-1 声明函']], 'name': '声明函'}
    last = {'heading_refs': [refs['9-3 业绩表']], 'name': '业绩表'}
    chapters = [first, last]
    if selection == 'crossing':
        first['body'] = [refs['9-1 声明函'], refs['不适用式样固定正文']]
        last['body'] = [refs['9-3 业绩表'], refs['须保留的另一表正文']]
    elif selection == 'excluded_heading':
        chapters.insert(1, {'heading_refs': [refs['9-2 额外声明（不适用）']], 'name': '额外声明'})
    else:
        chapters.insert(1, {'heading_refs': [refs['1. 附随文件']], 'name': '附随文件'})
    with pytest.raises(ModelContractError):
        decode_bid_format_outline({'chapters': chapters}, prepared)


SLICED_NAMES = [f'第{number}号承诺书' for number in range(1, 61)]


def _sliced_documents():
    page = '第六章 投标文件格式\n' + ''.join(f'附件{number} {name}\n致：招标人\n'
                                           for number, name in enumerate(SLICED_NAMES, start=1))
    return [{'document_id': 'tender', 'role': 'main', 'pages': [page], 'line_units': [[]],
             'unread_pages': [], 'ocr_issues': []}]


def _slice_execute(fail_slice=None):
    import json
    import re

    from docreader.llm.request import ModelWorkUnitExhausted

    sent = []

    async def execute(task, request, validate):
        assert task == 'directory_plan'
        lines = json.loads(request['messages'][1]['content'][0]['text'])['lines']
        sent.append(lines)
        if len(sent) == fail_slice:
            raise ModelWorkUnitExhausted(task)
        chapters = [{'heading_refs': [ref], 'name': match[1]} for ref, text in lines
                    if (match := re.fullmatch(r'附件\d+ (.+)', text.strip()))]
        return validate({'chapters': chapters})
    return execute, sent


async def test_bug331_an_oversized_bid_format_section_is_selected_in_slices(tmp_path):
    # Fits one request at the default window; 22,000 leaves about 4,200 input tokens, under half of the list.
    execute, sent = _slice_execute()
    artifacts = ArtifactStore(tmp_path)
    ref = await generate_document(
        artifacts, _snapshot(), RequestBudget(context_tokens=22000, model='unused'), execute,
        directory_documents=_sliced_documents(), outline_only=True,
        directory_contract='bid_format_source_outline_v1')
    outline = artifacts.read_json(ref)
    assert len(sent) > 1
    assert [ref for lines in sent for ref, _text in lines] == sorted(ref for lines in sent for ref, _text in lines)
    assert all(lines[0][1].startswith('附件') for lines in sent)
    assert [chapter['title'] for chapter in outline['chapters']] == SLICED_NAMES
    assert not [issue for issue in outline.get('issues', []) if issue['code'] == 'outline_directory_plan_failed']


async def test_bug331_a_failed_slice_keeps_the_others_and_is_named_pending(tmp_path):
    execute, sent = _slice_execute(fail_slice=2)
    artifacts = ArtifactStore(tmp_path)
    ref = await generate_document(
        artifacts, _snapshot(), RequestBudget(context_tokens=22000, model='unused'), execute,
        directory_documents=_sliced_documents(), outline_only=True,
        directory_contract='bid_format_source_outline_v1')
    outline = artifacts.read_json(ref)
    missed = {text.split(' ', 1)[1] for _ref, text in sent[1] if text.startswith('附件')}
    titles = [chapter['title'] for chapter in outline['chapters']]
    assert missed and not missed & set(titles)
    assert titles == [name for name in SLICED_NAMES if name not in missed]
    assert [issue for issue in outline['issues'] if issue['code'] == 'outline_directory_plan_failed']


def _format_document(document_id, role, page):
    return {'document_id': document_id, 'role': role, 'pages': [page], 'line_units': [[]],
            'unread_pages': [], 'ocr_issues': []}


MAIN_FORMAT = '第六章 投标文件格式\n附件1 投标函\n致：招标人\n附件2 报价表\n单价\n第七章 合同条款\n合同正文\n'
AMENDMENT_FORMAT = '补遗一\n投标文件格式\n附件1 投标函\n致：招标人（补遗）\n附件9 廉洁承诺书\n我方承诺\n'


async def _select_annexes(tmp_path, documents):
    import json
    import re

    sent = []

    async def execute(task, request, validate):
        lines = json.loads(request['messages'][1]['content'][0]['text'])['lines']
        sent.append(json.loads(request['messages'][1]['content'][0]['text']))
        return validate({'chapters': [{'heading_refs': [ref], 'name': match[1]} for ref, text in lines
                                      if (match := re.fullmatch(r'附件\d+ (.+)', text.strip()))]})
    artifacts = ArtifactStore(tmp_path)
    ref = await generate_document(
        artifacts, _snapshot(), RequestBudget(model='unused'), execute, directory_documents=documents,
        outline_only=True, directory_contract='bid_format_source_outline_v1')
    return artifacts.read_json(ref), sent


def _windows(outline):
    found = []
    def walk(chapters):
        for chapter in chapters:
            found.append((chapter['title'], {window['document_id'] for window in chapter.get('body_windows') or []}))
            walk(chapter.get('children') or [])
    walk(outline['chapters'])
    return found


async def test_bug332_an_amendment_format_section_is_selected_beside_the_main_one(tmp_path):
    main = _format_document('tender', 'main', MAIN_FORMAT)
    _alone, alone_sent = await _select_annexes(tmp_path / 'alone', [main])
    outline, sent = await _select_annexes(tmp_path / 'both', [main, _format_document('addendum', 'amendment',
                                                                                     AMENDMENT_FORMAT)])
    assert sent[0] == alone_sent[0] and len(sent) == 2
    assert [chapter['title'] for chapter in outline['chapters']] == ['投标函', '报价表', '投标函', '廉洁承诺书']
    blob = '\n'.join(_purposes(outline))
    assert '合同正文' not in blob and '致：招标人（补遗）' in blob
    codes = [(issue['code'], issue['detail']) for issue in outline['issues']]
    conflict, = [detail for code, detail in codes if code == 'outline_directory_format_conflict']
    assert '「投标函」' in conflict and '主件' in conflict and '补遗' in conflict
    sources = [detail for code, detail in codes if code == 'outline_directory_format_source']
    assert len(sources) == 2 and all('补遗（文档 addendum）' in detail for detail in sources)


async def test_bug332_a_format_section_only_in_an_attachment_is_used(tmp_path):
    outline, sent = await _select_annexes(tmp_path, [
        _format_document('tender', 'main', '第一章 招标公告\n格式另册提供\n'),
        _format_document('forms', 'attachment', MAIN_FORMAT)])
    assert len(sent) == 1
    assert [chapter['title'] for chapter in outline['chapters']] == ['投标函', '报价表']
    assert not [issue for issue in outline['issues'] if issue['code'].startswith('outline_directory_format_')]


async def test_bug337_bid_format_outline_builds_multi_level_hierarchy_and_filters_non_format_literal_pending(tmp_path):
    import json

    pages = [
        '第三章 评标办法\n评分表重复行\n',
        (
            '第六章 投标文件格式\n'
            '一、投标书\n致：招标人\n'
            '廉洁投标承诺书\n我方廉洁承诺\n'
            '五、投标保证金证明\n'
            '5-1 投标保证金采用电子保函形式（通宝函）\n保函正文\n'
            '5-2 投标保证金采用汇款形式\n汇款回单\n'
            '十二、实施团队人员情况\n'
            '（一）实施团队人员情况表\n人员表正文\n'
            '（二）人员简历表\n简历表正文\n'
        ),
    ]
    documents = [{
        'document_id': 'tender', 'role': 'main', 'pages': pages,
        'line_units': [[] for _ in pages], 'unread_pages': [],
        'ocr_issues': [
            {'code': 'outline_directory_literal_pending', 'detail': '第1页第2行的重复原字无法按来源位置对应，正文保留版式行并待核。'},
            {'code': 'outline_directory_literal_pending', 'detail': '第2页第3行的重复原字无法按来源位置对应，正文保留版式行并待核。'},
        ],
    }]

    async def execute(_task, request, validate):
        found = {text.strip(): ref for ref, text in json.loads(request['messages'][1]['content'][0]['text'])['lines']}
        return validate({'chapters': [
            {'heading_refs': [found['一、投标书']], 'label': '一、投标书', 'name': '投标书'},
            {'heading_refs': [found['廉洁投标承诺书']], 'parent_heading_ref': found['一、投标书'], 'name': '廉洁投标承诺书'},
            {'heading_refs': [found['五、投标保证金证明']], 'label': '五、投标保证金证明', 'name': '投标保证金证明'},
            {'heading_refs': [found['5-1 投标保证金采用电子保函形式（通宝函）']],
             'label': '5-1 投标保证金采用电子保函形式（通宝函）',
             'parent_heading_ref': found['五、投标保证金证明'], 'name': '投标保证金采用电子保函形式（通宝函）'},
            {'heading_refs': [found['5-2 投标保证金采用汇款形式']],
             'label': '5-2 投标保证金采用汇款形式',
             'parent_heading_ref': found['五、投标保证金证明'], 'name': '投标保证金采用汇款形式'},
            {'heading_refs': [found['十二、实施团队人员情况']], 'label': '十二、实施团队人员情况', 'name': '实施团队人员情况'},
            {'heading_refs': [found['（一）实施团队人员情况表']], 'label': '（一）实施团队人员情况表',
             'parent_heading_ref': found['十二、实施团队人员情况'], 'name': '实施团队人员情况表'},
            {'heading_refs': [found['（二）人员简历表']], 'label': '（二）人员简历表',
             'parent_heading_ref': found['十二、实施团队人员情况'], 'name': '人员简历表'},
        ]})

    _artifacts, outline = await _outline(
        tmp_path, documents=documents, execute=execute, directory_contract='bid_format_source_outline_v1')
    assert _children(outline) == [
        ('一、投标书', ['廉洁投标承诺书']),
        ('五、投标保证金证明', ['5-1 投标保证金采用电子保函形式（通宝函）', '5-2 投标保证金采用汇款形式']),
        ('十二、实施团队人员情况', ['（一）实施团队人员情况表', '（二）人员简历表']),
    ]
    literal_issues = [issue['detail'] for issue in outline['issues'] if issue['code'] == 'outline_directory_literal_pending']
    assert literal_issues == ['第2页第3行的重复原字无法按来源位置对应，正文保留版式行并待核。']


@pytest.mark.parametrize('case', ['invented_label', 'annex_label', 'forward_parent', 'unknown_parent', 'cover_parent'])
def test_bug337_bid_format_outline_rejects_invalid_label_and_parent_heading_ref(case):
    from docreader.llm.request import ModelContractError
    from docreader.tender.bid_format_outline import _Prepared, decode_bid_format_outline

    if case == 'cover_parent':
        pages = [
            '第六章 投标文件格式\n投标文件\n招标编号：A\n投标人名称：（盖章）\n日期：年 月 日\n',
            '一、投标书\n致：招标人\n',
        ]
        documents = [{
            'document_id': 'tender', 'role': 'main', 'pages': pages,
            'line_units': [[None, None, 'cover-grid', 'cover-grid', 'cover-grid'], [None, None]],
            'template_parts': {'cover-grid': {'selectors': []}},
        }]
        prepared = _Prepared(documents, _snapshot())
        refs = {text.strip(): ref for ref, text in prepared.sections[0]['payload']['lines']}
        chapters = [
            {'heading_refs': [refs['投标文件']], 'name': '投标文件'},
            {'heading_refs': [refs['一、投标书']], 'label': '一、投标书',
             'parent_heading_ref': refs['投标文件'], 'name': '投标书'},
        ]
    else:
        pages = [
            '第六章 投标文件格式\n附件1 一、投标书\n致：招标人\n5-1 保函\n保函正文\n二、报价表\n总价\n',
        ]
        prepared = _Prepared([{'document_id': 'tender', 'role': 'main', 'pages': pages}], _snapshot())
        refs = {text.strip(): ref for ref, text in prepared.sections[0]['payload']['lines']}
        if case == 'invented_label':
            chapters = [{'heading_refs': [refs['附件1 一、投标书']], 'label': '九、投标书', 'name': '投标书'}]
        elif case == 'annex_label':
            chapters = [{'heading_refs': [refs['附件1 一、投标书']], 'label': '附件1 一、投标书', 'name': '投标书'}]
        elif case == 'forward_parent':
            chapters = [
                {'heading_refs': [refs['附件1 一、投标书']], 'parent_heading_ref': refs['二、报价表'], 'name': '投标书'},
                {'heading_refs': [refs['二、报价表']], 'name': '报价表'},
            ]
        else:
            chapters = [
                {'heading_refs': [refs['5-1 保函']], 'parent_heading_ref': refs['附件1 一、投标书'], 'name': '保函'},
            ]
    with pytest.raises(ModelContractError):
        decode_bid_format_outline({'chapters': chapters}, prepared)


def test_bid_format_outline_accepts_split_two_line_numbered_label_and_form_name():
    from docreader.tender.bid_format_outline import _Prepared, decode_bid_format_outline

    pages = [
        '第六章 投标文件格式\n五、投标保证金证明\n5-1 投标保证金采用电子保函形式（通宝函）\n投标保函\n保函正文\n',
    ]
    prepared = _Prepared([{'document_id': 'tender', 'role': 'main', 'pages': pages}], _snapshot())
    refs = {text.strip(): ref for ref, text in prepared.sections[0]['payload']['lines']}
    plan = decode_bid_format_outline({'chapters': [
        {'heading_refs': [refs['五、投标保证金证明']], 'label': '五、投标保证金证明', 'name': '投标保证金证明'},
        {'heading_refs': [refs['5-1 投标保证金采用电子保函形式（通宝函）'], refs['投标保函']],
         'label': '5-1 投标保证金采用电子保函形式（通宝函）', 'name': '投标保函',
         'parent_heading_ref': refs['五、投标保证金证明']},
    ]}, prepared)
    assert plan['nodes'][1]['name'] == '5-1 投标保证金采用电子保函形式（通宝函）'
    assert plan['nodes'][1]['parent_id'] == plan['nodes'][0]['id']


def test_bid_format_outline_allows_blank_lines_between_wrapped_heading_refs_but_rejects_text_gaps():
    from docreader.llm.request import ModelContractError
    from docreader.tender.bid_format_outline import _Prepared, decode_bid_format_outline

    pages = [
        '第六章 投标文件格式\n五、投标保证金证明\n5-2 投标保证金采用汇款形式\n\n\n\n银行汇款回单扫描件\n正文行\n六、其他\n',
    ]
    prepared = _Prepared([{'document_id': 'tender', 'role': 'main', 'pages': pages}], _snapshot())
    refs = {text.strip(): ref for ref, text in prepared.sections[0]['payload']['lines']}
    plan = decode_bid_format_outline({'chapters': [
        {'heading_refs': [refs['五、投标保证金证明']], 'label': '五、投标保证金证明', 'name': '投标保证金证明'},
        {'heading_refs': [refs['5-2 投标保证金采用汇款形式'], refs['银行汇款回单扫描件']],
         'label': '5-2 投标保证金采用汇款形式', 'name': '银行汇款回单扫描件',
         'parent_heading_ref': refs['五、投标保证金证明']},
    ]}, prepared)
    assert plan['nodes'][1]['name'] == '5-2 投标保证金采用汇款形式'

    with pytest.raises(ModelContractError):
        decode_bid_format_outline({'chapters': [
            {'heading_refs': [refs['5-2 投标保证金采用汇款形式'], refs['六、其他']],
             'label': '5-2 投标保证金采用汇款形式', 'name': '其他'},
        ]}, prepared)


async def test_bug337_bid_format_outline_preserves_cross_slice_parent_context(tmp_path, monkeypatch):
    import json
    import re

    from docreader.llm.request import ModelWorkUnitExhausted

    from docreader.tender import authoring_pipeline

    # Force the parent/child boundary regardless of tokenizer or prompt length.
    fits = authoring_pipeline._fits
    monkeypatch.setattr(authoring_pipeline, '_fits', lambda budget, task, payload:
                        len(payload.get('lines', ())) <= 3 and fits(budget, task, payload))
    body_lines = ['第六章 投标文件格式', '附件1 投标函', '五、投标保证金证明',
                  '5-1 电子保函格式', '保函正文。', '附件2 报价表']
    documents = [{'document_id': 'tender', 'role': 'main', 'pages': ['\n'.join(body_lines) + '\n'],
                  'line_units': [[]], 'unread_pages': [], 'ocr_issues': []}]

    sent_payloads = []

    async def execute(_task, request, validate):
        payload = json.loads(request['messages'][1]['content'][0]['text'])
        sent_payloads.append(payload)
        lines = payload['lines']
        parent_ctx = {text.strip(): ref for ref, text in payload.get('parent_context') or []}
        in_slice = {text.strip(): ref for ref, text in lines}
        chapters = []
        for ref, text in lines:
            stripped = text.strip()
            if (m := re.fullmatch(r'附件\d+ (.+)', stripped)):
                chapters.append({'heading_refs': [ref], 'name': m[1]})
            elif stripped == '五、投标保证金证明':
                chapters.append({'heading_refs': [ref], 'label': '五、投标保证金证明', 'name': '投标保证金证明'})
            elif stripped == '5-1 电子保函格式':
                parent_ref = in_slice.get('五、投标保证金证明') or parent_ctx.get('五、投标保证金证明')
                chapters.append({'heading_refs': [ref], 'label': '5-1 电子保函格式', 'name': '电子保函格式',
                                 **({'parent_heading_ref': parent_ref} if parent_ref is not None else {})})
        return validate({'chapters': chapters})

    artifacts = ArtifactStore(tmp_path / 'ok')
    ref = await generate_document(
        artifacts, _snapshot(), RequestBudget(context_tokens=22000, model='unused'), execute,
        directory_documents=documents, outline_only=True, directory_contract='bid_format_source_outline_v1')
    outline = artifacts.read_json(ref)
    assert len(sent_payloads) > 1
    assert any(payload.get('parent_context') for payload in sent_payloads[1:])
    guarantee = next(ch for ch in outline['chapters'] if ch['title'] == '五、投标保证金证明')
    assert [child['title'] for child in guarantee['children']] == ['5-1 电子保函格式']

    # If the slice owning the parent fails, the child in a later slice degrades cleanly to top-level.
    parent_slice_idx = next(
        idx for idx, payload in enumerate(sent_payloads)
        if any(text.strip() == '五、投标保证金证明' for _ref, text in payload['lines'])
    )
    child_slice_idx = next(
        idx for idx, payload in enumerate(sent_payloads)
        if any(text.strip() == '5-1 电子保函格式' for _ref, text in payload['lines'])
    )
    assert parent_slice_idx != child_slice_idx
    calls = 0

    async def execute_fail_parent(task, request, validate):
        nonlocal calls
        current = calls
        calls += 1
        if current == parent_slice_idx:
            raise ModelWorkUnitExhausted(task)
        return await execute(task, request, validate)

    artifacts_fail = ArtifactStore(tmp_path / 'fail')
    ref_fail = await generate_document(
        artifacts_fail, _snapshot(), RequestBudget(context_tokens=22000, model='unused'), execute_fail_parent,
        directory_documents=documents, outline_only=True, directory_contract='bid_format_source_outline_v1')
    outline_fail = artifacts_fail.read_json(ref_fail)
    assert any(ch['title'] == '5-1 电子保函格式' for ch in outline_fail['chapters'])


async def test_bug298_host_plan_preserves_wrapped_headings_and_dedups_word_body(tmp_path):
    """BUG-298: host bid_format_plan joins wrapped titles, passes heading_lines, and rejects wrapped prose."""
    from docreader.tender.directory_compile import bid_format_plan

    pages = [
        '第六章 投标文件格式\n'
        '一、投标函\n'
        '一、投标人如在本次招标中存在以下情形之一\n'
        '甲方有权取消其中标资格\n'
        '投标函正文。\n'
        '八、投标人根据评标办法需要提供的其他商务\n'
        '及技术文件\n'
        '其他文件说明。\n'
        '九、省级以上监狱管理局(含新疆生产建设兵团)出具的属于监狱\n'
        '企业的证明文件\n'
        '证明文件正文。\n'
        '关于投标人符合中小企业政策要求的\n'
        '声明函（8A）\n'
        '中小企业声明正文。\n'
        '十、跨页说明（测试\n',
        '跨页尾行）\n'
        '跨页正文。\n',
    ]
    documents = [{'document_id': 'tender', 'role': 'main', 'pages': pages,
                  'line_units': [[] for _ in pages], 'unread_pages': [], 'ocr_issues': []}]

    host_plan = bid_format_plan(documents)
    assert host_plan is not None
    by_name = {node['name']: node for node in host_plan['nodes']}
    assert by_name['投标人根据评标办法需要提供的其他商务及技术文件']['heading_lines'] == 2
    assert by_name['省级以上监狱管理局(含新疆生产建设兵团)出具的属于监狱企业的证明文件']['heading_lines'] == 2
    assert by_name['关于投标人符合中小企业政策要求的声明函']['heading_lines'] == 2
    assert not any(node['heading']['page'] == 0 and node['heading']['line'] == 14 and node.get('heading_lines', 1) > 1
                   for node in host_plan['nodes'])

    artifacts, outline = await _outline(tmp_path, documents=documents)
    titles = _forms(outline)
    assert '投标函' in titles
    assert '投标人根据评标办法需要提供的其他商务及技术文件' in titles
    assert '省级以上监狱管理局(含新疆生产建设兵团)出具的属于监狱企业的证明文件' in titles
    assert '关于投标人符合中小企业政策要求的声明函' in titles
    assert not any('存在以下情形之一' in title for title in titles)

    exported = export_authoring_document(artifacts, outline, [])
    document = Document(BytesIO(artifacts.read_bytes(exported['docx_ref'])))
    texts = [paragraph.text.strip() for paragraph in document.paragraphs if paragraph.text.strip()]
    heading_texts = [
        paragraph.text.strip() for paragraph in document.paragraphs
        if paragraph.style is not None and str(paragraph.style.name).startswith('Heading')
    ]
    assert any('省级以上监狱管理局(含新疆生产建设兵团)出具的属于监狱企业的证明文件' in text for text in heading_texts)
    assert any('投标人根据评标办法需要提供的其他商务及技术文件' in text for text in heading_texts)
    assert '企业的证明文件' not in texts
    assert '及技术文件' not in texts
    assert '声明函（8A）' not in texts


def test_bug298_host_plan_wrapped_list_heading_keeps_full_window():
    """BUG-298: a wrapped list-entry heading retains all its lines in body_windows."""
    from docreader.tender.directory_compile import bid_format_plan, compile_directory

    pages = [
        '第六章 投标文件格式\n'
        '一、投标人根据评标办法需要提供的其他商务\n'
        '及技术文件\n'
        '二、投标函\n'
        '投标函正文。\n',
    ]
    documents = [{'document_id': 'tender', 'role': 'main', 'pages': pages,
                  'line_units': [[] for _ in pages], 'unread_pages': [], 'ocr_issues': []}]
    host_plan = bid_format_plan(documents)
    assert host_plan is not None
    wrapped = next(node for node in host_plan['nodes'] if node['name'] == '投标人根据评标办法需要提供的其他商务及技术文件')
    assert wrapped['heading_lines'] == 2
    assert wrapped['body_windows'] == [{'document_id': 'tender', 'page': 0, 'start_line': 1, 'end_line': 2}]
    compiled = compile_directory(host_plan, documents)
    form_titles = [chapter['title'] for chapter in compiled['chapters'] if chapter['title'] not in ('商务文件', '技术文件', '报价文件')]
    assert form_titles == [
        '投标人根据评标办法需要提供的其他商务及技术文件',
        '投标函',
    ]

def test_credit_clause_cannot_supply_a_heading_inside_a_sentence():
    from docreader.llm.request import ModelContractError
    from docreader.tender.bid_format_outline import _Prepared, decode_bid_format_outline

    lines = ["第六章 投标文件格式", "一、资格材料", "附件 8H",
             "查询企业记录，并核对投标人是否被相关网站或",
             "各级平台列入失信被执行人名单（须提供查询截图）。", "附件 9 报价表", "金额"]
    docs = [{"document_id": "main", "pages": ["\n".join(lines)], "role": "main"}]
    prepared = _Prepared(docs, None)
    refs = [prepared.refs.by_point[("main", 0, i)] for i in (1, 4, 5)]
    valid = [{"heading_refs": [refs[0]], "name": "资格材料"}, {"heading_refs": [refs[2]], "name": "报价表"}]
    bad = {"heading_refs": [refs[1]], "name": "失信被执行人名单"}
    with pytest.raises(ModelContractError):
        decode_bid_format_outline({"chapters": [valid[0], bad, valid[1]]}, prepared)
    good = decode_bid_format_outline({"chapters": valid}, prepared)
    assert good["nodes"][0]["body_windows"] == [{"document_id": "main", "page": 0, "start_line": 1, "end_line": 4}]


@pytest.mark.parametrize("line", ["1 报价表", "附表 B.1 报价表", "8D-1 表 报价表", "报价表（请按说明填报）"])
def test_independent_source_title_keeps_printed_prefix_and_note_forms(line):
    from docreader.tender.bid_format_outline import _Prepared, decode_bid_format_outline

    doc = {"document_id": "main", "pages": ["第六章 投标文件格式\n" + line], "role": "main"}
    prepared = _Prepared([doc], None)
    head = prepared.refs.by_point[("main", 0, 1)]
    plan = decode_bid_format_outline({"chapters": [{"heading_refs": [head], "name": "报价表"}]}, prepared)
    assert plan["nodes"][0]["name"] == "报价表"


@pytest.mark.parametrize(("line", "names", "label"), [
    ("（5）授权书（如需，格式供参考）", ["授权书"], "（5）授权书"),
    ("三、报价表（可按实际情况填写）", ["报价表"], "三、报价表"),
    ("A 分项价格表", ["分项价格表"], "A 分项价格表"),
    ("8B 表 股权结构表", ["股权结构表"], None),
    ("附件 9 开标表及报价承诺函", ["开标表", "报价承诺函"], None),
    ("资信证明（或银行证明）；商业信誉（专利、获奖及荣誉证明）。",
     ["资信证明（或银行证明）", "商业信誉（专利、获奖及荣誉证明）"], None),
])
def test_complete_source_titles_with_format_notes_or_separate_list_members(line, names, label):
    from docreader.tender.bid_format_outline import _Prepared, decode_bid_format_outline

    prepared = _Prepared([{"document_id": "m", "role": "main", "pages": ["第六章 投标文件格式\n" + line]}], None)
    head = prepared.refs.by_point[("m", 0, 1)]
    result = decode_bid_format_outline({"chapters": [
        {"heading_refs": [head], "name": name, "label": label} for name in names]}, prepared)
    assert [node["name"] for node in result["nodes"]] == ([label] if label else names)
