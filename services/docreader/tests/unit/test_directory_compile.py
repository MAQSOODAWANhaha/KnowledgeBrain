from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.tender.authoring_pipeline import generate_document
from docreader.tender.directory_compile import (
    DirectoryPlanRejected, compile_directory, compile_plans, documents_from_frozen, outline_from_compiled,
)
import pytest

from docreader.tender.document_execution import load_prompts
from test_authoring_pipeline_boundaries import payload as request_payload


def fixture_titles(payload, names=None):
    """Synthetic fixtures reuse their declared short names or explicit business aliases."""
    names = names or {}
    return [{'title_id': entry['title_id'], 'title': names.get(
        entry['adopted_source_name'], entry['adopted_source_name'])}
        for entry in payload.get('chapter_title_catalog', [])]


def test_layout_reading_uses_frozen_verified_checkbox_evidence(monkeypatch):
    from copy import deepcopy
    from docreader.tender import directory_compile as module

    pages = ['条款\n    \uf052不要求  \uf0a3要求\n', '保留空白\n']
    evidence = [{'page_ordinal': 0, 'char_index': index, 'raw_symbol': char, 'normalized_symbol': symbol,
                 'font_name': 'ABCDEF+Wingdings2,Bold', 'glyph_name': glyph, 'font_sha256': 'a' * 64}
                for index, (char, symbol, glyph) in enumerate([
                    ('\uf052', '☑', 'boxcheck'), ('\uf0a3', '□', 'box1')])]
    raw = {'source_contract': {'glyph_normalizations': evidence}, 'units': []}
    original = deepcopy(raw)
    monkeypatch.setattr(module, '_layout_pages', lambda *_: (list(pages), None))
    owner = [[None, 'native-source-table'], [None]]
    monkeypatch.setattr(module, '_bind_grid_lines', lambda text, *_: owner if text == pages else {})
    actual, owners, issues, unread = module._reading_pages(None, raw)
    assert actual == pages
    assert not issues and not unread
    assert owners == owner
    assert raw['source_contract'] == original['source_contract']
    assert any('☑' in item['text'] and '□' in item['text'] for item in raw['body_projection'])
    assert [len(page.splitlines()) for page in actual] == [len(page.splitlines()) for page in pages]


def test_verified_symbol_proofs_allow_equivalent_embedded_font_names(monkeypatch):
    from docreader.tender import directory_compile as module

    proofs = [{'page_ordinal': 0, 'char_index': index, 'raw_symbol': '\uf052', 'normalized_symbol': '☑',
               'font_name': name, 'glyph_name': 'boxcheck', 'font_sha256': 'a' * 64}
              for index, name in enumerate(['ABCDEF+Wingdings2,Bold', 'GHIJKL+Wingdings2'])]
    raw = {'source_contract': {'glyph_normalizations': proofs}, 'units': []}
    monkeypatch.setattr(module, '_layout_pages', lambda *_: (['\uf052条款甲\n\uf052条款乙'], None))
    pages, _, issues, _ = module._reading_pages(None, raw)
    assert pages == ['\uf052条款甲\n\uf052条款乙'] and not issues
    assert any('☑条款甲' in item['text'] for item in raw['body_projection'])


@pytest.mark.parametrize('problem', [
    'missing', 'font', 'glyph', 'meaning', 'count', 'extra_proof', 'mixed', 'page',
    'duplicate_index', 'missing_font_sha', 'bad_font_sha', 'unknown',
    'font_none', 'font_int', 'glyph_list', 'boolean_page', 'non_dict'])
def test_unverified_layout_symbol_is_preserved_with_a_manual_review_exit(monkeypatch, problem):
    from docreader.tender import directory_compile as module

    proof = {'page_ordinal': 0, 'char_index': 1, 'raw_symbol': '\uf052', 'normalized_symbol': '☑',
             'font_name': 'Wingdings2', 'glyph_name': 'boxcheck', 'font_sha256': 'a' * 64}
    evidence = [proof]
    page = '条款\n\uf052不要求'
    if problem == 'missing':
        evidence = []
    elif problem == 'font':
        proof['font_name'] = 'OtherSymbolFont'
    elif problem == 'glyph':
        proof['glyph_name'] = 'different'
    elif problem == 'meaning':
        proof['normalized_symbol'] = '□'
    elif problem == 'count':
        page += '\n\uf052其他条款'
    elif problem == 'extra_proof':
        evidence.append({**proof, 'char_index': 2})
    elif problem == 'mixed':
        page += '\n\uf052其他条款'
        evidence.append({**proof, 'font_name': 'OtherSymbolFont'})
    elif problem == 'page':
        proof['page_ordinal'] = 1
    elif problem == 'duplicate_index':
        page += '\n\uf052其他条款'
        evidence.append(dict(proof))
    elif problem == 'missing_font_sha':
        proof.pop('font_sha256')
    elif problem == 'bad_font_sha':
        proof['font_sha256'] = 'not-a-source-font-sha'
    elif problem == 'unknown':
        page = '条款\n\uf099不要求'
    elif problem == 'font_none':
        proof['font_name'] = None
    elif problem == 'font_int':
        proof['font_name'] = 12
    elif problem == 'glyph_list':
        proof['glyph_name'] = []
    elif problem == 'boolean_page':
        proof['page_ordinal'] = False
    elif problem == 'non_dict':
        evidence = [None]
    raw = {'source_contract': {'glyph_normalizations': evidence}, 'units': []}
    monkeypatch.setattr(module, '_layout_pages', lambda *_: ([page], None))
    actual, _, issues, _ = module._reading_pages(None, raw)
    assert actual == [page]
    assert len(issues) == 1 and issues[0]['code'] == 'outline_directory_symbol_unconfirmed'
    assert '第1页' in issues[0]['detail'] and '原件' in issues[0]['detail']


def point(page, line, document_id='tender'):
    return {'document_id': document_id, 'page': page, 'line': line}


def window(page, start, end=None, document_id='tender'):
    return {'document_id': document_id, 'page': page, 'start_line': start, 'end_line': start if end is None else end}


def docs(pages, *documents):
    return [{'document_id': 'tender', 'role': 'main', 'pages': pages}, *documents]


def node(identity, page, line, parent=None, content=None, document_id='tender'):
    value = {'id': identity, 'heading': point(page, line, document_id), 'parent_id': parent}
    if content is not None:
        value['content_start'] = point(*content, document_id)
    return value


def item(node_id, depth='self', entry=None, presentation='section', condition=None, clause=None):
    return {'node_id': node_id, 'entry': entry, 'depth': depth, 'presentation': presentation,
            'condition': condition, 'clause': clause or []}


def plan(nodes, items, entries=(), basis=(), excluded=(), source_only=(), title=None):
    return {'nodes': [{'response_kind': 'form', **value} for value in nodes],
            'composition': {'priority_basis': list(basis), 'entries': list(entries)},
            'scopes': [{'title': title, 'items': items}], 'excluded': list(excluded), 'source_only': list(source_only)}


def entry(page, line, excluded_by=(), document_id='tender'):
    return {'name': point(page, line, document_id), 'excluded_by': list(excluded_by)}


def test_composition_order_beats_the_model_array_and_a_decorated_form_keeps_the_listed_name():
    pages = ['优先采用下列组成。\n1. 投标函\n2. 身份证明\n', '投标函的格式\n致招标人\n身份证明\n姓名\n']
    compiled = compile_directory(plan(
        [node('proof', 1, 2), node('letter', 1, 0)],
        [item('proof', entry=1), item('letter', entry=0)],
        [entry(0, 1), entry(0, 2)], [window(0, 0)]), docs(pages))
    assert [chapter['title'] for chapter in compiled['chapters']] == ['投标函', '身份证明']
    assert compiled['chapters'][0]['text'] == '投标函的格式\n致招标人'
    assert 'outline_directory_name' not in {issue['code'] for issue in compiled['issues']}


def test_irregular_source_names_publish_as_one_business_name():
    pages = ['按本清单。\n（1）投 标 函\n', '1、投标函的格式\n致招标人\n']
    compiled = compile_directory(plan(
        [node('letter', 1, 0)], [item('letter', entry=0)], [entry(0, 1)], [window(0, 0)]), docs(pages))
    assert compiled['chapters'][0]['title'] == '投标函'
    assert not any(issue['code'] == 'outline_directory_name' for issue in compiled['issues'])


def test_a_trailing_list_mark_is_not_a_different_chapter_name():
    pages = ['组成。\n投标函；\n', '投标函\n正文\n']
    compiled = compile_directory(plan([node('letter', 1, 0)], [item('letter', entry=0)], [entry(0, 1)]), docs(pages))
    assert compiled['chapters'][0]['title'] == '投标函'
    assert compiled['chapters'][0]['listed_title'] == '投标函'
    assert not any(issue['code'] == 'outline_directory_name' for issue in compiled['issues'])


def test_different_wording_keeps_the_form_heading_and_records_the_difference():
    pages = ['按本清单。\n开标一览表\n', '报价一览表\n单价\n']
    compiled = compile_directory(plan(
        [node('quote', 1, 0)], [item('quote', entry=0)], [entry(0, 1)], [window(0, 0)]), docs(pages))
    assert compiled['chapters'][0]['title'] == '报价一览表'
    assert compiled['chapters'][0]['listed_title'] == '开标一览表'
    assert compiled['chapters'][0]['heading_title'] == '报价一览表'
    assert compiled['issues'][0]['code'] == 'outline_directory_name'
    assert '开标一览表' in compiled['issues'][0]['detail'] and '报价一览表' in compiled['issues'][0]['detail']


def test_invented_name_condition_and_title_field_are_rejected():
    pages = ['组成。\n投标函\n如有\n', '投标函\n正文\n']
    base = plan([node('letter', 1, 0)], [item('letter', entry=0, condition=window(0, 2))],
                [entry(0, 1)], [window(0, 0)])
    renamed = plan([{**node('letter', 1, 0), 'name': '综述'}], base['scopes'][0]['items'],
                   base['composition']['entries'], base['composition']['priority_basis'])
    with pytest.raises(DirectoryPlanRejected, match='not a fragment'):
        compile_directory(renamed, docs(pages))
    with pytest.raises(DirectoryPlanRejected, match='outside the document'):
        compile_directory(plan(base['nodes'], [item('letter', entry=0, condition=window(0, 9))],
                               base['composition']['entries'], base['composition']['priority_basis']), docs(pages))
    with pytest.raises(DirectoryPlanRejected, match='title'):
        compile_directory({**base, 'title': '模型起的目录名'}, docs(pages))
    compiled = compile_directory(base, docs(pages))
    assert compiled['chapters'][0]['condition'] == '如有'
    outline = outline_from_compiled(_snapshot(), compiled)
    assert '如有' in [target['purpose'] for target in outline['chapters'][0]['targets']]


def test_scope_title_becomes_one_shared_volume_and_does_not_rename_the_chapter():
    pages = ['投标文件\n', '投标函\n正文\n身份证明\n姓名\n']
    compiled = compile_directory(plan(
        [node('proof', 1, 2), node('letter', 1, 0)],
        [item('proof'), item('letter')], title=point(0, 0)), docs(pages))
    outline = outline_from_compiled(_snapshot(), compiled)
    volume = outline['chapters'][0]
    assert volume['title'] == '投标文件' and volume['is_volume'] is True
    assert volume['chapter_id'] == 'chapter-1'
    assert [(chapter['chapter_id'], chapter['title']) for chapter in volume['children']] == [
        ('chapter-1.1', '投标函'), ('chapter-1.2', '身份证明')]
    same = compile_directory(plan([node('letter', 1, 0)], [item('letter')], title=point(1, 0)), docs(pages))
    reused = outline_from_compiled(_snapshot(), same)['chapters'][0]
    assert reused['title'] == '投标函' and reused['is_volume'] is True
    assert reused['children'] == []
    assert reused['targets'][0]['purpose'] == '投标函\n正文\n身份证明\n姓名'


def test_content_presentation_keeps_source_order_without_a_numbered_heading(tmp_path):
    from io import BytesIO
    from docx import Document
    from docreader.tender.authoring_export import export_authoring_document

    pages = ['封面\n某某项目\n', '投标函\n正文\n']
    compiled = compile_directory(plan(
        [node('cover', 0, 0), node('letter', 1, 0)],
        [item('cover', presentation='content'), item('letter', presentation='content')]), docs(pages))
    outline = outline_from_compiled(_snapshot(), compiled)
    by_title = {chapter['title']: chapter for chapter in outline['chapters']}
    assert by_title['封面']['presentation_role'] == 'content'
    assert by_title['投标函']['presentation_role'] == 'content'
    units = [{'schema_version': 2, 'task_id': 'task', 'chapter_id': chapter['chapter_id'],
              'target_ids': [target['target_id'] for target in chapter['targets']], 'status': 'placeholder',
              'blocks': [], 'cell_fills': [], 'row_fills': [], 'attachments': [], 'gaps': []}
             for chapter in outline['chapters']]
    artifacts = ArtifactStore(tmp_path)
    document = Document(BytesIO(artifacts.read_bytes(
        export_authoring_document(artifacts, outline, units)['docx_ref'])))
    assert [chapter['title'] for chapter in outline['chapters']] == ['封面', '投标函']
    headings = [paragraph.text for paragraph in document.paragraphs if paragraph.style.name.startswith('Heading')]
    assert not any(text.endswith('封面') or text.endswith('投标函') for text in headings)
    body = [paragraph.text for paragraph in document.paragraphs]
    assert '某某项目' in body and '正文' in body
    assert '（原格式待补）' not in body and '（本部分内容待补）' not in body


def test_every_entry_and_node_needs_one_decision_and_cycles_are_rejected():
    pages = ['组成。\n投标函\n不适用\n', '投标函\n正文\n另一章\n']
    nodes = [node('letter', 1, 0), node('other', 1, 2)]
    entries = [entry(0, 1), entry(0, 2)]
    with pytest.raises(DirectoryPlanRejected, match='without a decision'):
        compile_directory(plan(nodes, [item('letter', entry=0)], entries, [window(0, 0)], source_only=['other']), docs(pages))
    with pytest.raises(DirectoryPlanRejected, match='without a decision'):
        compile_directory(plan([node('letter', 1, 0), node('other', 1, 2)], [item('letter')]), docs(pages))
    with pytest.raises(DirectoryPlanRejected, match='parent cycle'):
        compile_directory(plan([node('letter', 1, 0, 'other'), node('other', 1, 2, 'letter')],
                               [item('letter', 'subtree')]), docs(pages))
    compiled = compile_directory(plan(
        nodes, [item('letter', entry=0)], [entry(0, 1), entry(0, 2, [window(0, 2)])],
        [window(0, 0)], source_only=['other']), docs(pages))
    assert [chapter['id'] for chapter in compiled['chapters']] == ['letter']


def test_contents_indentation_overrides_only_inside_the_same_root_and_page():
    pages = ['第二部分 ........ 2\n  附表 ........ 2\n    表一 ........ 2\n', '第二部分\n正文\n附表\n表一\n格子\n']
    compiled = compile_directory(plan(
        [node('part', 1, 0), node('tables', 1, 2, 'part'), node('one', 1, 3, 'part')],
        [item('part', 'subtree')]), docs(pages))
    parent = {chapter['id']: chapter['parent_id'] for chapter in compiled['chapters']}
    assert parent['tables'] == 'part' and parent['one'] == 'tables'
    assert '格子' in next(chapter['text'] for chapter in compiled['chapters'] if chapter['id'] == 'one')
    assert '格子' not in next(chapter['text'] for chapter in compiled['chapters'] if chapter['id'] == 'tables')
    assert _contents_parent(['总册 ... 1\n  分册 ... 1\n', '    小节 ... 1\n'], 'section') == 'book'
    assert _contents_parent(['总册 ... 1\n  分册 ... 1\n    小节 ... 1\n'], 'section') == 'part'


def test_a_contents_title_in_another_root_does_not_move_the_node():
    pages = ['其他部分 ... 2\n  表格 ... 2\n', '其他部分\n表格\n正文\n']
    compiled = compile_directory(plan(
        [node('other', 1, 0), node('form', 1, 1)], [item('form')], source_only=['other']), docs(pages))
    assert compiled['chapters'][0]['parent_id'] is None


def test_page_numbers_and_repeated_headers_are_not_copied_and_the_next_heading_ends_the_body():
    pages = ['招标文件\n1\n投标函\n金额\n2\n', '招标文件\n身份证明\n姓名\n']
    compiled = compile_directory(plan(
        [node('letter', 0, 2), node('proof', 1, 1)], [item('letter'), item('proof')]), docs(pages))
    assert compiled['chapters'][0]['text'] == '投标函\n金额'
    assert '招标文件' not in compiled['chapters'][0]['text']
    assert '1' not in compiled['chapters'][0]['text'].splitlines()


def test_content_cannot_grow_child_chapters_and_a_rejected_plan_has_no_placeholder():
    pages = ['封面\n签章\n后一章\n']
    with pytest.raises(DirectoryPlanRejected, match='content cannot have child chapters'):
        compile_directory(plan([node('cover', 0, 0), node('next', 0, 2, 'cover')],
                               [item('cover', 'children', presentation='content')]), docs(pages))
    rejected = compile_plans([{'nodes': [], 'title': '模型目录'}], docs(pages))
    assert rejected['chapters'] == []
    assert rejected['issues'][0]['code'] == 'outline_directory_rejected'


def test_an_extra_grandchild_keeps_its_parent_from_the_same_plan():
    pages = ['技术文件\n硬件技术规格\n技术参数表\n']
    first = plan([node('tech', 0, 0)], [item('tech')])
    second = plan([node('tech', 0, 0), node('hardware', 0, 1, 'tech'), node('table', 0, 2, 'hardware')],
                  [item('tech', 'subtree')])
    merged = compile_plans([first, second], docs(pages))
    table = next(chapter for chapter in merged['chapters'] if chapter['id'] == 'table')
    assert table['parent_id'] == 'hardware'


def test_an_extra_child_stays_under_the_parent_both_plans_share():
    pages = ['技术文件\n总则\n范围\n']
    first = plan([node('tech', 0, 0)], [item('tech')])
    second = plan([node('tech', 0, 0), node('general', 0, 1, 'tech')], [item('tech', 'children')])
    merged = compile_plans([first, second], docs(pages))
    general = next(chapter for chapter in merged['chapters'] if chapter['id'] == 'general')
    assert general['parent_id'] == 'tech'
    assert any('正文范围不一致' in issue['detail'] for issue in merged['issues'])


def test_the_more_complete_plan_is_kept_and_a_conflict_does_not_guess_a_parent():
    pages = ['按本清单。\n投标函\n身份证明\n']
    full = plan([], [item(None, entry=0), item(None, entry=1)], [entry(0, 1), entry(0, 2)], [window(0, 0)])
    short = plan([], [item(None, entry=0)], [entry(0, 2)], [window(0, 0)])
    chosen = compile_plans([short, full], docs(pages))
    assert [chapter['title'] for chapter in chosen['chapters']] == ['投标函', '身份证明']
    assert chosen['issues'][-1]['code'] == 'outline_directory_gap'

    body = ['投标函\n正文\n身份证明\n姓名\n']
    siblings = plan([node('letter', 0, 0), node('proof', 0, 2)], [item('letter'), item('proof')])
    nested = plan([node('proof', 0, 2), node('letter', 0, 0, 'proof')], [item('proof', 'children')])
    kept = compile_plans([siblings, nested], docs(body))
    letter = next(chapter for chapter in kept['chapters'] if chapter['id'] == 'letter')
    assert letter['parent_id'] is None
    assert any(issue['code'] == 'outline_directory_conflict' for issue in kept['issues'])
    assert [chapter['id'] for chapter in compile_directory(siblings, docs(body))['chapters']] == ['letter', 'proof']


def test_a_heading_split_by_an_open_parenthesis_stays_one_title():
    pages = ['（1）股权说明（需明确\n出资比例）。\n']
    compiled = compile_directory(plan([node('equity', 0, 0)], [item('equity')]), docs(pages))
    assert compiled['chapters'][0]['title'] == '股权说明（需明确出资比例）。'


def test_one_invalid_depth_does_not_discard_the_plan():
    pages = ['投标函\n正文\n']
    value = plan([node('letter', 0, 0)], [item('letter'),
                 {'node_id': 'letter', 'entry': None, 'depth': 'content', 'presentation': 'section'}])
    compiled = compile_directory(value, docs(pages))
    assert [chapter['title'] for chapter in compiled['chapters']] == ['投标函']
    assert any(issue['code'] == 'outline_directory_extra' for issue in compiled['issues'])


def test_an_adoption_without_a_node_or_entry_is_skipped():
    pages = ['投标函\n正文\n']
    value = plan([node('letter', 0, 0)], [item('letter'), {'node_id': None, 'entry': None, 'depth': 'self', 'presentation': 'section'}])
    compiled = compile_directory(value, docs(pages))
    assert [chapter['title'] for chapter in compiled['chapters']] == ['投标函']
    assert any(issue['code'] == 'outline_directory_extra' for issue in compiled['issues'])


def test_an_appendix_binding_uses_the_cited_line_and_the_node_id():
    pages = ['报价见附表C。\n', '附表C 开标一览表\n项目名称\n']
    value = plan([node('sheet', 1, 0)], [item('sheet')])
    value['format_bindings'] = [{'id': 'annex-c', 'source': window(0, 0),
                                 'response_node_id': 'sheet', 'reference_node_ids': []}]
    compiled = compile_directory(value, docs(pages))
    chapter = compiled['chapters'][0]
    assert chapter['title'] == '附表C 开标一览表'
    assert chapter['format_bindings'] == [{'id': 'annex-c', 'source': '报价见附表C。', 'reference_ids': []}]
    assert any(issue['code'] == 'outline_directory_binding' and '报价见附表C' in issue['detail']
               for issue in compiled['issues'])


def test_self_does_not_swallow_a_child_that_is_adopted_on_its_own():
    pages = ['资格审查资料\n基本情况表\n投标人名称\n近年财务状况表\n']
    compiled = compile_directory(plan(
        [node('qual', 0, 0), node('base', 0, 1, 'qual'), node('finance', 0, 3, 'qual')],
        [item('qual'), item('base'), item('finance')]), docs(pages))
    assert [chapter['title'] for chapter in compiled['chapters']] == ['资格审查资料', '基本情况表', '近年财务状况表']
    assert compiled['chapters'][1]['parent_id'] == 'qual'


def test_clause_windows_are_copied_from_the_source_lines():
    pages = ['投标函\n须提供授权委托书\n']
    compiled = compile_directory(plan(
        [node('letter', 0, 0)],
        [item('letter', clause=[window(0, 1)])]), docs(pages))
    assert compiled['chapters'][0]['obligations'] == '须提供授权委托书'


def test_a_repeated_composition_entry_keeps_the_extra_chapter_by_its_own_heading():
    pages = ['投标文件组成\n投标函\n', '投标函\n格式\n身份证明\n姓名\n']
    compiled = compile_directory(plan(
        [node('letter', 1, 0), node('proof', 1, 2)],
        [item('letter', entry=0), item('proof', entry=0)],
        [entry(0, 1)]), docs(pages))
    assert [chapter['title'] for chapter in compiled['chapters']] == ['投标函', '身份证明']
    assert any(issue['code'] == 'outline_directory_name' and '重复引用' in issue['detail'] for issue in compiled['issues'])


def test_main_composition_does_not_need_a_priority_basis():
    pages = ['投标文件组成\n投标函\n', '投标函\n格式\n']
    compiled = compile_directory(plan([node('letter', 1, 0)], [item('letter', entry=0)], [entry(0, 1)]), docs(pages))
    assert [chapter['title'] for chapter in compiled['chapters']] == ['投标函']


def test_revision_documents_do_not_silently_replace_or_cross_the_main_document():
    tender = ['按招标文件组成。\n投标函\n', '投标函\n正文\n']
    amendment = ['补遗规定以本清单为准。\n投标函\n授权委托书\n']
    documents = [*docs(tender), {'document_id': 'addendum', 'role': 'amendment', 'pages': amendment}]
    main_plan = plan([node('letter', 1, 0)], [item('letter', entry=0)], [entry(0, 1)], [window(0, 0)])
    compiled = compile_directory(main_plan, documents)
    assert [chapter['title'] for chapter in compiled['chapters']] == ['投标函']
    assert compiled['chapters'][0]['text'] == '投标函\n正文'
    assert any(issue['code'] == 'outline_directory_revision_unreviewed' and 'addendum' in issue['detail']
               for issue in compiled['issues'])

    mixed = plan([], [item(None, entry=0), item(None, entry=1)],
                 [entry(0, 1), entry(0, 1, document_id='addendum')],
                 [window(0, 0), window(0, 0, document_id='addendum')])
    with pytest.raises(DirectoryPlanRejected, match='one document'):
        compile_directory(mixed, documents)
    with pytest.raises(DirectoryPlanRejected, match='priority basis in that document'):
        compile_directory(plan([], [item(None, entry=0)], [entry(0, 1, document_id='addendum')], [window(0, 0)]), documents)

    revised = plan(
        [node('letter', 1, 0), node('proxy', 0, 2, document_id='addendum')],
        [item('letter', entry=0), item('proxy', entry=1)],
        [entry(0, 1, document_id='addendum'), entry(0, 2, document_id='addendum')],
        [window(0, 0, document_id='addendum')])
    adopted = compile_directory(revised, documents)
    assert [chapter['title'] for chapter in adopted['chapters']] == ['投标函', '授权委托书']
    assert adopted['chapters'][0]['text'] == '投标函\n正文'
    assert '授权委托书' in adopted['chapters'][1]['text']
    assert '正文' not in adopted['chapters'][1]['text']

    with pytest.raises(DirectoryPlanRejected, match='heading document'):
        compile_directory(plan([node('letter', 1, 0) | {'content_start': point(0, 0, 'addendum')}],
                               [item('letter')]), documents)


def test_contents_in_another_document_do_not_move_a_node():
    tender = ['投标函\n']
    amendment = ['投标函 ... 1\n  附表 ... 1\n', '投标函\n附表\n']
    documents = [*docs(tender), {'document_id': 'addendum', 'role': 'amendment', 'pages': amendment}]
    compiled = compile_directory(plan(
        [node('letter', 0, 0), node('table', 1, 1, document_id='addendum')],
        [item('letter'), item('table')], source_only=[]), documents)
    assert next(chapter['parent_id'] for chapter in compiled['chapters'] if chapter['id'] == 'letter') is None


def _contents_parent(toc_pages, child):
    body = '总册\n分册\n小节\n'
    pages = [*toc_pages, body]
    body_page = len(toc_pages)
    compiled = compile_directory(plan(
        [node('book', body_page, 0), node('part', body_page, 1, 'book'), node('section', body_page, 2, 'book')],
        [item('book', 'subtree')]), docs(pages))
    return next(chapter['parent_id'] for chapter in compiled['chapters'] if chapter['id'] == child)


def _snapshot():
    return {'schema_version': 2, 'run_id': 'run', 'source_version': 1, 'domain_revision': 1}


async def test_compiled_directory_is_saved_without_a_model_call_or_a_placeholder_chapter(tmp_path):
    pages = ['按本清单。\n1. 投标函\n2. 身份证明\n', '投标函的格式\n致招标人\n身份证明\n姓名\n']
    plans = [plan([node('proof', 1, 2), node('letter', 1, 0)],
                  [item('proof', entry=1), item('letter', entry=0)],
                  [entry(0, 1), entry(0, 2)], [window(0, 0)])]
    documents = docs(pages)

    async def execute(*_args):
        raise AssertionError('directory compilation must not call the model')

    artifacts = ArtifactStore(tmp_path)
    budget = RequestBudget(model='fixture', context_tokens=131072, output_reserve=16384)
    reference = await generate_document(
        artifacts, _snapshot(), budget, execute, directory_plans=plans, directory_documents=documents, outline_only=True)
    outline = artifacts.read_json(reference)
    assert [(chapter['chapter_id'], chapter['title']) for chapter in outline['chapters']] == [
        ('chapter-1', '投标函'), ('chapter-2', '身份证明')]
    letter = outline['chapters'][0]
    assert letter['targets'][0]['target_id'] == 'chapter-1:target-1'
    assert letter['targets'][0]['purpose'] == '投标函的格式\n致招标人'
    assert letter['targets'][0]['manual_fill'] is True
    assert letter['targets'][0]['support_origins'] == ['tender']

    rejected = artifacts.read_json(await generate_document(
        artifacts, _snapshot(), budget, execute, directory_plans=[{'title': '模型目录'}],
        directory_documents=documents, outline_only=True))
    assert rejected['chapters'] == []
    assert any(issue['code'] == 'outline_directory_rejected' for issue in rejected['issues'])
    assert any(issue['code'] == 'outline_empty' for issue in rejected['issues'])


def test_table_numbers_and_repeated_page_headers_stay_inside_chapter():
    unit_a, unit_b = 'page:0:table:0', 'page:1:table:0'
    pages = ['技术要求\n指标项\n1\n', '指标项\n2\n']
    documents = [{'document_id': 'tender', 'role': 'main', 'pages': pages,
                  'line_units': [[None, unit_a, unit_a], [unit_b, unit_b]],
                  'template_parts': {unit: {'source_ref': {'source_index_ref': {'key': unit}, 'unit_id': unit},
                                             'selectors': []} for unit in (unit_a, unit_b)}}]
    chapter = compile_directory(plan([node('tech', 0, 0)], [item('tech')]), documents)['chapters'][0]
    assert [part['source_ref']['unit_id'] for part in chapter['template_parts']] == [unit_a, unit_b]
    assert [(ref['page'], ref['line']) for ref in chapter['lines'] if ref.get('unit_id') == unit_b] == [(1, 0), (1, 1)]


def test_export_places_table_between_original_introduction_and_note(tmp_path):
    from docx.oxml.ns import qn
    from docreader.tender.authoring_export import export_authoring_document
    from test_authoring_export import document, grid, source_ref

    artifacts = ArtifactStore(tmp_path)
    ref = source_ref(artifacts, grid=grid())
    unit_id = ref['unit_id']
    documents = [{'document_id': 'tender', 'role': 'main',
                  'pages': ['人员表\n表前说明\n人员安排表\n负责人\n证书编号\n表后注意事项\n'],
                  'line_units': [[None, None, unit_id, unit_id, unit_id, None]],
                  'template_parts': {unit_id: {'source_ref': ref, 'selectors': []}}}]
    compiled = compile_directory(plan([node('form', 0, 0)], [item('form')]), documents)
    outline = outline_from_compiled(_snapshot(), compiled)
    doc = document(artifacts, export_authoring_document(artifacts, outline, []))
    body = list(doc.element.body)
    before = next(i for i, element in enumerate(body) if '表前说明' in ''.join(element.xpath('.//w:t/text()')))
    table = next(i for i, element in enumerate(body) if element.tag == qn('w:tbl'))
    after = next(i for i, element in enumerate(body) if '表后注意事项' in ''.join(element.xpath('.//w:t/text()')))
    assert before < table < after
    assert sum('证书编号' in paragraph.text for paragraph in doc.paragraphs) == 0
    assert doc.tables[0].cell(2, 0).text == '证书编号'


def test_cleared_grid_text_is_still_readable_by_row(tmp_path):
    from test_structural_packing import source

    artifacts = ArtifactStore(tmp_path)
    def cell(row, column, text):
        return {'row': row, 'column': column, 'text': text, 'row_span': 1, 'col_span': 1}

    index = source([{'text': '', 'grid': {'row_count': 2, 'column_count': 2, 'cells': [
        cell(0, 1, '格式'), cell(0, 0, '投标函'), cell(1, 0, '项目经理'), cell(1, 1, '')]}}],
        document_id='tender')
    documents = documents_from_frozen(artifacts, {'documents': [_freeze(artifacts, index, 'main')]})
    assert documents[0]['pages'] == ['投标函\t格式\n项目经理\t']
    outline = outline_from_compiled(_snapshot(), compile_directory(
        plan([node('form', 0, 0)], [item('form')]), documents))
    target = outline['chapters'][0]['targets'][0]
    refs = target['template_refs']
    assert refs[0]['source_ref']['unit_id'] == index.raw['units'][0]['unit_id']
    assert refs[0]['selectors'] == []
    from io import BytesIO
    from docx import Document
    from docreader.tender.authoring_export import export_authoring_document

    exported = export_authoring_document(artifacts, outline, [{
        'schema_version': 2, 'task_id': 'task', 'chapter_id': outline['chapters'][0]['chapter_id'],
        'target_ids': [target['target_id']], 'status': 'placeholder', 'blocks': [], 'cell_fills': [],
        'row_fills': [], 'attachments': [], 'gaps': []}])
    document = Document(BytesIO(artifacts.read_bytes(exported['docx_ref'])))
    assert len(document.tables) == 1
    assert [cell.text for cell in document.tables[0].rows[0].cells] == ['投标函', '格式']
    assert [cell.text for cell in document.tables[0].rows[1].cells] == ['项目经理', '']
    assert not any('投标函\t格式' in paragraph.text for paragraph in document.paragraphs)


def test_a_page_the_plan_did_not_adopt_is_still_copied():
    pages = ['1. 总则\n投标人应阅读全部招标文件。\n', '投标函\n正文\n']
    compiled = compile_plans([plan([node('letter', 1, 0)], [item('letter')])], docs(pages))
    assert [chapter['title'] for chapter in compiled['chapters']] == ['投标函']


def test_a_continued_table_page_stays_under_the_heading_before_the_table():
    first, second, notice = 'page:0:table:0', 'page:1:table:0', 'page:2:table:0'
    pages = ['投标人须知前附表\n条款号 条款名称\n1.1.2 招标人\n',
             '实质性要求和条\n1.11.1 详见评标办法\n',
             '附件一：问题澄清通知\n请澄清\n']
    documents = [{'document_id': 'tender', 'role': 'main', 'pages': pages, 'ocr_issues': [],
                  'line_units': [[None] * 3, [None] * 2, [None] * 2],
                  'template_parts': {unit: {'source_ref': {'source_index_ref': {'key': 'x'}, 'unit_id': unit}, 'selectors': []}
                                     for unit in (first, second, notice)},
                  'grids': [
                      {'unit_id': first, 'page': 0, 'rows': [['条款号', '条款名称'], ['1.1.2', '招标人']]},
                      {'unit_id': second, 'page': 1, 'rows': [['实质性要求和条件', '详见评标办法']]},
                      {'unit_id': notice, 'page': 2, 'rows': [['请澄清']]}]}]
    compiled = compile_plans([{'nodes': [], 'title': '模型目录'}], documents)
    assert compiled['chapters'] == []


def test_a_table_outside_the_adopted_chapters_is_still_kept():
    unit_id = 'page:0:table:0'
    pages = ['投标人须知前附表\n条款号 条款名称\n1.1.2 招标人\n', '投标函\n正文\n']
    documents = [{'document_id': 'tender', 'role': 'main', 'pages': pages,
                  'line_units': [[None, unit_id, unit_id], [None] * 2], 'ocr_issues': [],
                  'template_parts': {unit_id: {'source_ref': {'source_index_ref': {'key': 'x'}, 'unit_id': unit_id},
                                               'selectors': []}},
                  'grids': [{'unit_id': unit_id, 'page': 0, 'rows': [['条款号', '条款名称'], ['1.1.2', '招标人']]}]}]
    compiled = compile_plans([plan([node('letter', 1, 0)], [item('letter')])], documents)
    from docreader.tender.directory_compile import unplaced_tables

    assert [chapter['title'] for chapter in compiled['chapters']] == ['投标函']
    assert unplaced_tables(compiled, documents)[0]['page'] == 0


async def test_unplaced_source_pages_do_not_trigger_parameter_calls(monkeypatch):
    import docreader.tender.authoring_pipeline as pipeline
    from docreader.tender.directory_compile import _chapter

    source_pages = ['\n'.join(f'原文{index}' for index in range(30))]
    adopted = _chapter('adopted', '技术要求', None, 'section', ('tender', 0, 0), None,
                       '\n'.join(source_pages[0].splitlines()), '', None, None)
    adopted['response_kind'] = 'narrative'
    unplaced = _chapter('extra', '未归属', None, 'section', ('tender', 0, 0), None,
                        '\n'.join(source_pages[0].splitlines()), '', None, None)
    unplaced['unplaced'] = True
    compiled = {'chapters': [adopted, unplaced], 'issues': []}
    calls = []

    async def ask(_execute, _budget, task, payload, validate):
        calls.append((task, payload))
        return validate({'directory_material': {'assignments': [], 'chapter_titles': fixture_titles(payload)}, 'directory_requirement': {'assignments': []},
                         'directory_classification': {'items': []}, 'directory_parameter': {'chapters': []}}[str(task)])

    monkeypatch.setattr(pipeline, '_ask', ask)
    await pipeline._request_directory_details(compiled, docs(source_pages), None, None)
    requested = [payload['chapter']['id'] for task, payload in calls if str(task) == 'directory_parameter']
    assert requested == ['adopted']


def test_a_table_between_two_chapters_stays_with_the_preceding_one():
    from docreader.tender.directory_compile import apply_materials

    unit_id = 'page:0:table:0'
    pages = ['防病毒软件技术要求\n产品类别 产品名称\n桌面 防病毒\n威胁行为防护软件技术要求\n',
             '投标产品技术性能指标的详细描述\n请填写。\n']
    documents = [{'document_id': 'tender', 'role': 'main', 'pages': pages,
                  'line_units': [[None, unit_id, unit_id, None], [None, None]],
                  'template_parts': {unit_id: {'source_ref': {'source_index_ref': {'key': 'x'}, 'unit_id': unit_id},
                                               'selectors': []}},
                  'grids': [{'unit_id': unit_id, 'page': 0, 'rows': [['产品类别', '产品名称'], ['桌面', '防病毒']]}]}]
    nodes = [node('technical', 1, 0), node('antivirus', 0, 0, 'technical'), node('threat', 0, 3, 'technical')]
    fresh = compile_plans([plan(nodes, [item('technical'), item('antivirus'), item('threat')])], documents)
    apply_materials(fresh, documents, [{'chapter_id': 'technical', 'document_id': 'tender', 'page': 0,
                                        'start_line': 1, 'end_line': 2}])
    antivirus = next(chapter for chapter in fresh['chapters'] if chapter['id'] == 'antivirus')
    technical = next(chapter for chapter in fresh['chapters'] if chapter['id'] == 'technical')
    assert [part['source_ref']['unit_id'] for part in antivirus.get('template_parts') or []] == [unit_id]
    assert not technical.get('template_parts') and not technical.get('material_segments')


def test_a_shared_table_keeps_one_parent_whichever_child_the_model_names():
    from docreader.tender.directory_compile import apply_materials

    unit_id = 'page:0:table:0'
    pages = ['产品清单\n产品类别 产品名称\n桌面 防病毒\n防病毒软件技术要求\n指标\n',
             '威胁行为防护软件技术要求\n指标\n', '投标产品技术性能指标的详细描述\n请填写。\n']
    documents = [{'document_id': 'tender', 'role': 'main', 'pages': pages,
                  'line_units': [[None, unit_id, unit_id, None, None], [None, None], [None, None]],
                  'template_parts': {unit_id: {'source_ref': {'source_index_ref': {'key': 'x'}, 'unit_id': unit_id},
                                               'selectors': []}},
                  'grids': [{'unit_id': unit_id, 'page': 0, 'rows': [['产品类别', '产品名称'], ['桌面', '防病毒']]}]}]
    nodes = [node('technical', 2, 0), node('antivirus', 0, 3, 'technical'), node('threat', 1, 0, 'technical')]
    compiled = compile_plans([plan(nodes, [item('technical'), item('antivirus'), item('threat')])], documents)
    for named in ('antivirus', 'threat'):
        fresh = compile_plans([plan(nodes, [item('technical'), item('antivirus'), item('threat')])], documents)
        apply_materials(fresh, documents, [{'chapter_id': named, 'document_id': 'tender', 'page': 0,
                                            'start_line': 1, 'end_line': 2}])
        owner = next(chapter for chapter in fresh['chapters'] if chapter['id'] == 'technical')
        assert owner.get('material_segments')
        assert not any(chapter['id'] == named and chapter.get('material_segments') for chapter in fresh['chapters'])
    assert compiled['chapters']


def test_partial_material_table_is_not_exported_as_a_complete_grid():
    from docreader.tender.directory_compile import apply_materials

    unit_id = 'page:0:table:0'
    documents = [{'document_id': 'tender', 'role': 'main',
                  'pages': ['表头\n内容一\n内容二\n', '报价表\n正文\n'],
                  'line_units': [[unit_id, unit_id, unit_id], [None, None]],
                  'template_parts': {unit_id: {'source_ref': {'source_index_ref': {'key': 'x'}, 'unit_id': unit_id},
                                               'selectors': []}},
                  'grids': [{'unit_id': unit_id, 'page': 0, 'rows': [['表头'], ['内容一'], ['内容二']]}]}]
    compiled = compile_plans([plan([node('price', 1, 0)], [item('price')])], documents)
    apply_materials(compiled, documents, [{'chapter_id': 'price', 'document_id': 'tender',
                                           'page': 0, 'start_line': 0, 'end_line': 1}])
    price = next(chapter for chapter in compiled['chapters'] if chapter['id'] == 'price')
    assert not price.get('material_segments')
    assert any('表格' in issue['detail'] and '待核' in issue['detail'] for issue in compiled['issues'])


def test_material_assignment_moves_complete_source_table_into_existing_chapter(tmp_path):
    from docreader.tender.authoring_export import export_authoring_document
    from docreader.tender.directory_compile import apply_materials
    from test_authoring_export import document, grid, source_ref

    artifacts = ArtifactStore(tmp_path)
    ref = source_ref(artifacts, grid=grid())
    unit_id = ref['unit_id']
    documents = [{'document_id': 'tender', 'role': 'main',
                  'pages': ['人员安排表\n负责人\n证书编号\n', '报价表\n报价说明\n'],
                  'line_units': [[unit_id, unit_id, unit_id], [None, None]],
                  'template_parts': {unit_id: {'source_ref': ref, 'selectors': []}},
                  'grids': [{'unit_id': unit_id, 'page': 0, 'rows': [['人员安排表'], ['负责人'], ['证书编号']]}]}]
    compiled = compile_plans([plan([node('price', 1, 0)], [item('price')])], documents)
    apply_materials(compiled, documents, [{'chapter_id': 'price', 'document_id': 'tender',
                                           'page': 0, 'start_line': 0, 'end_line': 2}])
    outline = outline_from_compiled(_snapshot(), compiled)
    price = next(chapter for chapter in outline['chapters'] if chapter['title'] == '报价表')
    assert sum(len(target.get('template_refs', [])) for target in price['targets']) == 1
    result = export_authoring_document(artifacts, outline, [])
    doc = document(artifacts, result)
    assert len(doc.tables) == 1
    assert doc.tables[0].cell(0, 0).text == '人员安排表'
    assert doc.tables[0].cell(0, 0)._tc is doc.tables[0].cell(0, 1)._tc


def test_two_documents_can_have_the_same_table_unit_id():
    unit = 'page:0:table:0'
    documents = []
    for document_id, role in [('main', 'main'), ('attachment', 'attachment')]:
        documents.append({'document_id': document_id, 'role': role, 'pages': ['表格\n字段 内容\n'],
                          'line_units': [[None, unit]],
                          'template_parts': {unit: {'source_ref': {'source_index_ref': {'key': document_id},
                                                                  'unit_id': unit}, 'selectors': []}},
                          'grids': [{'unit_id': unit, 'page': 0, 'rows': [['字段', '内容']]}]})
    selected = plan([node('form', 0, 0, document_id='main')], [item('form')])
    compiled = compile_plans([selected], documents)
    from docreader.tender.directory_compile import unplaced_tables

    refs = [part for chapter in compiled['chapters'] for part in chapter.get('template_parts', [])]
    assert {part['source_ref']['source_index_ref']['key'] for part in refs} == {'main'}
    assert any(item['document_id'] == 'attachment' for item in unplaced_tables(compiled, documents))


def test_table_before_chapter_is_not_attached_to_later_chapter():
    unit_id = 'page:0:table:0'
    pages = ['产品清单\n产品类别 产品名称\n桌面软件 防病毒软件\n防病毒软件技术要求\n桌面软件 防病毒软件\n其他要求\n']
    documents = [{'document_id': 'tender', 'role': 'main', 'pages': pages,
                  'line_units': [[None, unit_id, unit_id, None, None, None]], 'ocr_issues': [],
                  'template_parts': {unit_id: {'source_ref': {'source_index_ref': {'key': 'x'}, 'unit_id': unit_id},
                                               'selectors': []}},
                  'grids': [{'unit_id': unit_id, 'page': 0,
                             'rows': [['产品类别', '产品名称'], ['桌面软件', '防病毒软件']]}]}]
    compiled = compile_directory(plan([node('tech', 0, 3)], [item('tech')]), documents)
    assert not compiled['chapters'][0].get('template_parts')


def test_a_table_on_a_chapter_page_stays_even_when_the_header_is_split():
    unit_id = 'page:0:table:0'
    pages = ['综合报价表\n序 可研产品\n号 名称\n1 防病毒软件\n设备总价（元）\n']
    documents = [{'document_id': 'tender', 'role': 'main', 'pages': pages,
                  'line_units': [[None] * 5], 'ocr_issues': [],
                  'template_parts': {unit_id: {'source_ref': {'source_index_ref': {'key': 'x'}, 'unit_id': unit_id},
                                               'selectors': []}},
                  'grids': [{'unit_id': unit_id, 'page': 0, 'rows': [
                      ['序号', '可研产品名称', '所投产品名称', '规格型号'],
                      ['防病毒软件']]}]}]
    chapter = compile_directory(plan([node('quote', 0, 0)], [item('quote')]), documents)['chapters'][0]
    assert chapter['template_parts'][0]['source_ref']['unit_id'] == unit_id


def test_a_wrapped_header_still_attaches_the_native_table():
    unit_id = 'page:0:table:0'
    pages = ['投标函格式\n序号 评价条款 投标响应情况\n证明资料所在文件\n及页码\n注：逐条响应。\n']
    documents = [{'document_id': 'tender', 'role': 'main', 'pages': pages,
                  'line_units': [[None] * 5], 'ocr_issues': [],
                  'template_parts': {unit_id: {'source_ref': {'source_index_ref': {'key': 'x'}, 'unit_id': unit_id},
                                               'selectors': []}},
                  'grids': [{'unit_id': unit_id, 'page': 0,
                             'rows': [['序号', '评价条款', '投标响应情况', '证明资料所在文件及页码']]}]}]
    chapter = compile_directory(plan([node('form', 0, 0)], [item('form')]), documents)['chapters'][0]
    assert chapter['template_parts'][0]['source_ref']['unit_id'] == unit_id
    assert '投标函格式' in chapter['text'] and '注：逐条响应。' in chapter['text']
    # Wrapped lines without confirmed ownership remain verbatim, even when the native table is also rendered.
    assert '证明资料所在文件' in chapter['text'] and '序号' in chapter['text']


def test_scanned_text_uses_vision_ocr_before_tesseract(tmp_path, monkeypatch):
    import docreader.tender.directory_compile as directory_compile

    calls = []
    monkeypatch.setattr(directory_compile, '_tesseract', lambda data: calls.append(data) or '本地识别')
    artifacts = ArtifactStore(tmp_path)
    image_ref = artifacts.put_bytes(b'not-an-image')
    vision = _scan_index(artifacts, {'text': '视觉识别的投标函', 'status': 'partial', 'issues': []}, image_ref)
    local = _scan_index(artifacts, {'text': '', 'status': 'unresolved', 'issues': []}, image_ref)
    vision_pages = documents_from_frozen(artifacts, {'documents': [_freeze(artifacts, vision, 'main')]})
    assert vision_pages[0]['pages'] == ['视觉识别的投标函']
    assert calls == []
    assert vision_pages[0]['ocr_issues'][0]['code'] == 'outline_directory_ocr_unconfirmed'
    assert vision_pages[0]['unread_pages'] == [0]
    local_pages = documents_from_frozen(artifacts, {'documents': [_freeze(artifacts, local, 'main')]})
    assert local_pages[0]['pages'] == ['本地识别']
    assert calls == [b'not-an-image']
    assert local_pages[0]['ocr_issues'][0]['code'] == 'outline_directory_ocr_unconfirmed'
    assert 'tesseract' in local_pages[0]['ocr_issues'][0]['detail']
    compiled = compile_directory(plan([node('scan', 0, 0)], [item('scan')]), local_pages)
    assert compiled['chapters'][0]['text'] == '本地识别'
    assert any(issue['code'] == 'outline_directory_ocr_unconfirmed' for issue in compiled['issues'])


def test_direct_text_is_not_replaced_by_ocr_and_a_short_heading_is_not_a_scan(tmp_path, monkeypatch):
    import docreader.tender.directory_compile as directory_compile
    from docreader.tender.directory_compile import sparse_page_issues
    from docreader.tender.evidence.references import SourceIndex
    from test_structural_packing import source

    calls = []
    monkeypatch.setattr(directory_compile, '_tesseract', lambda data: calls.append(data) or '印章误识别')
    artifacts = ArtifactStore(tmp_path)
    image_ref = artifacts.put_bytes(b'logo')
    index = source([{'text': '投标函格式如下，投标人应按照本格式填写并签字盖章。'}, {'text': ''}], document_id='tender')
    text, image = index.raw['units']
    image['kind'] = 'image_region'
    image['locator']['page_ordinal'] = text['locator']['page_ordinal']
    image['locator']['image_key'] = 'logo'
    index.raw['images'] = {'logo': image_ref}
    index.raw['ocr'] = {image['unit_id']: {'text': '不应覆盖原文', 'status': 'complete', 'issues': []}}
    heading = {'unit_id': 'heading', 'kind': 'section', 'text': '生态环境部', 'grid': None,
               'locator': {'locator_kind': 'document'}}
    index.raw['units'].append(heading)
    documents = documents_from_frozen(artifacts, {'documents': [_freeze(artifacts, SourceIndex(index.document, index.raw), 'main')]})
    assert calls == []
    assert documents[0]['pages'][0] == '投标函格式如下，投标人应按照本格式填写并签字盖章。'
    assert '不应覆盖原文' not in '\n'.join(documents[0]['pages'])
    assert documents[0]['ocr_issues'] == []
    assert documents[0]['unread_pages'] == []
    assert sparse_page_issues(documents) == []


def test_pdf_layout_keeps_toc_indent_and_a_short_title_is_not_recognized(tmp_path, monkeypatch):
    from pypdf import PdfWriter
    from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject
    import docreader.tender.directory_compile as directory_compile
    from test_structural_packing import source

    writer = PdfWriter()
    font = writer._add_object(DictionaryObject({
        NameObject('/Type'): NameObject('/Font'), NameObject('/Subtype'): NameObject('/Type1'),
        NameObject('/BaseFont'): NameObject('/Helvetica')}))
    resources = DictionaryObject({NameObject('/Font'): DictionaryObject({NameObject('/F1'): font})})
    for stream_bytes in (
        b'BT /F1 14 Tf 70 700 Td (Volume One) Tj ET',
        b'BT /F1 12 Tf 60 720 Td (Contents) Tj 24 -20 Td (1. Technical Proposal) Tj ET',
    ):
        page = writer.add_blank_page(width=400, height=800)
        page[NameObject('/Resources')] = resources
        stream = DecodedStreamObject()
        stream.set_data(stream_bytes)
        page[NameObject('/Contents')] = writer._add_object(stream)
    pdf_path = tmp_path / 'toc-layout.pdf'
    with pdf_path.open('wb') as handle:
        writer.write(handle)

    calls = []
    monkeypatch.setattr(directory_compile, '_tesseract', lambda data: calls.append(True) or '不应识别')
    artifacts = ArtifactStore(tmp_path)
    index = source([{'text': '解析单元不应代替版式'}], document_id='tender')
    index.raw['original_ref'] = artifacts.put_bytes(pdf_path.read_bytes())
    documents = documents_from_frozen(artifacts, {'documents': [_freeze(artifacts, index, 'main')]})
    assert any(line.startswith('   1.') for line in documents[0]['pages'][1].splitlines())
    assert '解析单元不应代替版式' not in '\n'.join(documents[0]['pages'])
    assert any('Volume One' in page for page in documents[0]['pages'])
    assert calls == []


def test_a_table_row_split_across_layout_lines_is_kept_by_page_rules(tmp_path):
    from pypdf import PdfWriter
    from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject
    from test_structural_packing import source

    content = b'''BT /F1 12 Tf 70 700 Td (Name) Tj 0 -16 Td (Value) Tj ET
60 720 m 280 720 l S
60 670 m 280 670 l S
60 640 m 280 640 l S
60 640 m 60 720 l S
180 640 m 180 720 l S
280 640 m 280 720 l S
'''
    writer = PdfWriter()
    page = writer.add_blank_page(width=400, height=800)
    font = writer._add_object(DictionaryObject({
        NameObject('/Type'): NameObject('/Font'), NameObject('/Subtype'): NameObject('/Type1'),
        NameObject('/BaseFont'): NameObject('/Helvetica')}))
    page[NameObject('/Resources')] = DictionaryObject({NameObject('/Font'): DictionaryObject({NameObject('/F1'): font})})
    stream = DecodedStreamObject()
    stream.set_data(content)
    page[NameObject('/Contents')] = writer._add_object(stream)
    target = tmp_path / 'ruled.pdf'
    with target.open('wb') as handle:
        writer.write(handle)
    artifacts = ArtifactStore(tmp_path)
    index = source([{'text': '', 'grid': {'row_count': 1, 'column_count': 2, 'cells': [
        {'row': 0, 'column': 0, 'text': 'Name', 'row_span': 1, 'col_span': 1},
        {'row': 0, 'column': 1, 'text': 'Value', 'row_span': 1, 'col_span': 1}]}}], document_id='tender')
    index.raw['original_ref'] = artifacts.put_bytes(target.read_bytes())
    documents = documents_from_frozen(artifacts, {'documents': [_freeze(artifacts, index, 'main')]})
    assert 'Name' in documents[0]['pages'][0] and 'Value' in documents[0]['pages'][0]
    assert documents[0]['line_units'][0].count('unit:0') == 2


def test_a_physical_page_without_usable_text_is_recognized(tmp_path, monkeypatch):
    from pypdf import PdfWriter
    import docreader.tender.directory_compile as directory_compile
    from test_structural_packing import source

    monkeypatch.setattr(directory_compile, '_tesseract', lambda data: '扫描到的投标函' if data else '')
    artifacts = ArtifactStore(tmp_path)
    writer = PdfWriter()
    writer.add_blank_page(width=300, height=300)
    target = tmp_path / 'blank.pdf'
    with target.open('wb') as handle:
        writer.write(handle)
    index = source([{'text': ''}], document_id='tender')
    index.raw['original_ref'] = artifacts.put_bytes(target.read_bytes())
    documents = documents_from_frozen(artifacts, {'documents': [_freeze(artifacts, index, 'main')]})
    assert documents[0]['pages'] == ['扫描到的投标函']
    assert 'tesseract' in documents[0]['ocr_issues'][0]['detail']


def _scan_index(artifacts, ocr, image_ref):
    from docreader.tender.evidence.references import SourceIndex
    from test_structural_packing import source

    index = source([{'text': ''}], document_id='tender')
    unit = index.raw['units'][0]
    unit['kind'] = 'image_region'
    unit['locator']['image_key'] = 'scan'
    index.raw['images'] = {'scan': image_ref}
    index.raw['ocr'] = {unit['unit_id']: ocr}
    return SourceIndex(index.document, index.raw)


def test_frozen_documents_stay_separate_and_keep_registered_order(tmp_path):
    from test_structural_packing import source

    artifacts = ArtifactStore(tmp_path)
    main = source([{'text': '第一页甲\n第一页乙'}, {'text': '第二页'}], document_id='tender')
    amendment = source([{'text': '补遗清单'}], document_id='addendum')
    frozen = {'documents': [_freeze(artifacts, main, 'main'), _freeze(artifacts, amendment, 'amendment')]}
    documents = documents_from_frozen(artifacts, frozen)
    assert [(item['document_id'], item['role'], item['pages']) for item in documents] == [
        ('tender', 'main', ['第一页甲\n第一页乙', '第二页']),
        ('addendum', 'amendment', ['补遗清单'])]


async def test_registered_documents_request_two_location_plans_and_do_not_organize(tmp_path):
    pages = ['按本清单。\n投标函\n']
    documents = docs(pages)
    calls = []

    async def execute(task, request, validate):
        calls.append(task)
        if task in {'directory_requirement', 'directory_material'}:
            return validate({'assignments': [], **({'chapter_titles': fixture_titles(request_payload(request))}
                                                   if task == 'directory_material' else {})})
        if task == 'directory_classification':
            return validate({'items': []})
        assert '投标函' in request['messages'][1]['content'][0]['text']
        return validate(plan([{**node('letter', 0, 1), 'body_windows': [window(0, 1)]}],
                             [item('letter')], source_only=[]))

    artifacts = ArtifactStore(tmp_path)
    outline = artifacts.read_json(await generate_document(
        artifacts, _snapshot(), RequestBudget(model='fixture', context_tokens=131072, output_reserve=16384,
                                              prompts={**load_prompts(), 'directory_plan': '只返回位置。'}),
        execute, directory_documents=documents, outline_only=True))
    assert calls[:3] == ['directory_plan', 'directory_plan', 'directory_material']
    assert {'directory_requirement', 'directory_classification'} <= set(calls)
    assert outline['chapters'][0]['title'] == '投标函'
    assert outline['chapters'][0]['chapter_id'] == 'chapter-1'


async def test_outline_from_frozen_sources_uses_directory_plans(tmp_path):
    from test_structural_packing import source

    artifacts = ArtifactStore(tmp_path)
    main = source([{'text': '按本清单。\n投标函'}], document_id='tender')
    amendment = source([{'text': '补遗未引用'}], document_id='addendum')
    frozen_ref = artifacts.put_json({'documents': [
        _freeze(artifacts, main, 'main'), _freeze(artifacts, amendment, 'amendment')]})
    calls = []

    async def execute(task, _request, validate):
        calls.append(task)
        if task == 'directory_requirement':
            return validate({'assignments': [{'document_id': 'tender', 'page': 0, 'start_line': 1, 'end_line': 1,
                                              'scope': 'chapter', 'chapter_ids': ['letter']}]})
        if task == 'directory_material':
            return validate({'assignments': [], 'chapter_titles': fixture_titles(request_payload(_request))})
        if task == 'directory_classification':
            return validate({'items': [{'kind': 'service', 'document_id': 'tender', 'page': 0,
                                        'start_line': 0, 'end_line': 0}]})
        return validate(plan([{**node('letter', 0, 1), 'body_windows': [window(0, 1)]}], [item('letter')]))

    snapshot = {**_snapshot(), 'frozen_ref': frozen_ref}
    outline = artifacts.read_json(await generate_document(
        artifacts, snapshot, RequestBudget(model='fixture', context_tokens=131072, output_reserve=16384,
                                           prompts={**load_prompts(), 'directory_plan': '只返回位置。'}),
        execute, outline_only=True))
    assert calls[:3] == ['directory_plan', 'directory_plan', 'directory_material']
    assert {'directory_requirement', 'directory_classification'} <= set(calls)
    assert outline['chapters'][0]['title'] == '投标函'
    assert outline['chapters'][0]['targets'][-1]['purpose'] == '投标函'
    assert any(issue['code'] == 'outline_directory_revision_unreviewed' for issue in outline['issues'])


def test_materials_do_not_repeat_template_lines_and_classification_quotes_source():
    from docreader.tender.directory_compile import apply_classification, apply_materials, reading_payload

    documents = [{'document_id': 'tender', 'role': 'main', 'pages': ['投标函\n须提供授权']}]
    compiled = {'chapters': [{'id': 'letter', 'title': '投标函', 'parent_id': None, 'presentation': 'section',
                              'text': '投标函', 'obligations': '',
                              'lines': [{'document_id': 'tender', 'page': 0, 'line': 0}]}], 'issues': []}
    apply_materials(compiled, documents, [
        {'chapter_id': 'letter', 'document_id': 'tender', 'page': 0, 'start_line': 0, 'end_line': 1}])
    assert compiled['chapters'][0]['materials'] == ['须提供授权']
    apply_classification(compiled, documents, [
        {'kind': 'service', 'document_id': 'tender', 'page': 0, 'start_line': 1, 'end_line': 1}])
    assert compiled['issues'][-1]['detail'] == '服务：须提供授权'
    assert reading_payload(documents, {'source_version': 3, 'domain_revision': 4})['source_version'] == 3


def test_directory_decisions_are_listed_for_confirmation(tmp_path):
    from io import BytesIO
    from zipfile import ZipFile
    from docreader.tender.authoring_export import export_authoring_document

    artifacts = ArtifactStore(tmp_path)
    outline = {'chapters': [{'chapter_id': 'chapter-1', 'title': '投标函', 'targets': [], 'children': [],
                             'is_volume': False, 'presentation_role': 'body'}],
               'issues': [{'code': 'outline_directory_classification', 'detail': '服务：须提供授权', 'rule_refs': []}]}
    exported = export_authoring_document(artifacts, outline, [])
    with ZipFile(BytesIO(artifacts.read_bytes(exported['package_ref']))) as archive:
        notes = archive.read('提交要求与待办.md').decode()
    assert '目录待确认' in notes and '服务：须提供授权' in notes


def test_requirement_blocks_keep_pages_intact_and_host_copies_only_new_lines():
    from docreader.tender.directory_compile import apply_requirements, note_missing_requirements, requirement_blocks

    long_page = '\n'.join('行' for _ in range(6))
    separate = [{'document_id': 'tender', 'role': 'main', 'pages': [long_page, 'a\nb\nc\nd']}]
    assert [len(block) for block in requirement_blocks(separate, limit=6)] == [1, 1]
    packed = [{'document_id': 'tender', 'role': 'main', 'pages': ['甲\n乙', '丙\n丁']}]
    assert len(requirement_blocks(packed, limit=4)) == 1
    compiled = {'chapters': [{'id': 'letter', 'title': '投标函', 'parent_id': None, 'presentation': 'section',
                              'text': '投标函', 'obligations': '',
                              'lines': [{'document_id': 'tender', 'page': 0, 'line': 0}]}], 'issues': []}
    apply_requirements(compiled, packed, [
        {'document_id': 'tender', 'page': 0, 'start_line': 0, 'end_line': 1, 'scope': 'chapter', 'chapter_ids': ['letter']}])
    assert compiled['chapters'][0]['requirements'] == ['乙']
    shared = {'chapters': [
        {'id': 'a', 'title': '甲', 'parent_id': None, 'presentation': 'section', 'text': '', 'obligations': ''},
        {'id': 'b', 'title': '乙', 'parent_id': 'a', 'presentation': 'section', 'text': '', 'obligations': ''}],
        'issues': []}
    apply_requirements(shared, packed, [
        {'document_id': 'tender', 'page': 1, 'start_line': 0, 'end_line': 0, 'scope': 'chapters', 'chapter_ids': ['a', 'b']}])
    note_missing_requirements(shared)
    assert [chapter['requirements'] for chapter in shared['chapters']] == [['丙'], ['丙']]
    assert any(issue['code'] == 'outline_directory_shared_requirement' and issue['detail'] == '丙' for issue in shared['issues'])
    assert not any(issue['code'] == 'outline_directory_requirement_missing' and '乙' in issue['detail'] for issue in shared['issues'])


def test_parameter_view_shows_the_source_line_text():
    from docreader.tender.directory_compile import parameter_view
    documents = [{'document_id': 'tender', 'pages': ['指标项\n防病毒软件技术要求']}]
    chapter = {'lines': [{'document_id': 'tender', 'page': 0, 'line': 1}]}
    assert parameter_view(chapter, documents) == [
        {'document_id': 'tender', 'page': 0, 'line': 1, 'text': '防病毒软件技术要求'}]


def test_parameter_expansion_starts_at_the_content_line_and_short_chapters_stay_whole():
    from docreader.tender.directory_compile import apply_parameters

    body = '\n'.join(f'行{number}' for number in range(25))
    documents = [{'document_id': 'tender', 'role': 'main', 'pages': [body]}]
    compiled = {'chapters': [{'id': 'spec', 'title': '技术参数', 'parent_id': None, 'presentation': 'section',
                              'text': body, 'obligations': ''}], 'issues': []}
    apply_parameters(compiled, documents, [{'chapter_id': 'spec', 'items': [
        {'document_id': 'tender', 'page': 0, 'start_line': 0, 'end_line': 3, 'content_start_line': 1}]}])
    child = compiled['chapters'][1]
    assert child['parent_id'] == 'spec' and child['title'] == '行0' and child['text'] == '行1\n行2\n行3'
    short = {'chapters': [{'id': 'brief', 'title': '简述', 'parent_id': None, 'presentation': 'section',
                           'text': '只有一行', 'obligations': ''}], 'issues': []}
    apply_parameters(short, documents, [{'chapter_id': 'brief', 'items': [
        {'document_id': 'tender', 'page': 0, 'start_line': 0, 'end_line': 0, 'content_start_line': 0}]}])
    assert len(short['chapters']) == 1 and short['issues'][0]['code'] == 'outline_directory_parameter_short'


def _freeze(artifacts, index, role):
    raw_ref = artifacts.put_json(index.raw)
    return {**index.document, 'role': role, 'source_index_ref': artifacts.put_json(index.descriptor(raw_ref))}
