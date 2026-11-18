"""Nesting cannot replace bid composition order with the format book's page order."""

from copy import deepcopy

from docreader.tender.directory_compile import compile_directory, outline_from_compiled
from test_directory_compile import docs, entry, item, node, plan, point, window


def test_volume_children_keep_compiled_composition_order_instead_of_form_page_order():
    documents = docs(['报价文件\n开标价格表\n投标价格表', '投标价格表\n金额：________', '开标价格表\n总价：________'])
    raw = plan([{**node('price', 1, 0), 'body_windows': [window(1, 0, 1)]},
                {**node('opening', 2, 0), 'body_windows': [window(2, 0, 1)]}],
               [item('price', entry=1), item('opening', entry=0)],
               entries=[entry(0, 1), entry(0, 2)], title=point(0, 0))
    compiled = compile_directory(raw, documents)
    assert [chapter['title'] for chapter in compiled['chapters']] == ['开标价格表', '投标价格表']
    outline = outline_from_compiled({'schema_version': 2, 'run_id': 'order', 'source_version': 1,
                                     'domain_revision': 1}, compiled)
    volume = outline['chapters'][0]
    assert volume['title'] == '报价文件' and volume['is_volume']
    assert [chapter['title'] for chapter in volume['children']] == ['开标价格表', '投标价格表']


def grouped_plan():
    documents = docs(['商务投标文件\n技术投标文件\n报价文件',
                      '投标封面\n商务原式\n技术原式\n报价原式\n技术补充\n无标题材料',
                      '技术补充\n技术原式'])
    raw = plan([{**node(identity, 1, line), 'body_windows': []}
                for line, identity in enumerate(('cover', 'business', 'technical', 'price', 'extra', 'plain'))], [],
               entries=[entry(2, 0), entry(2, 1)])
    raw['source_only'] = ['plain']
    raw['scopes'] = [
        {'title': None, 'items': [item('cover', presentation='cover')]},
        {'title': point(0, 0), 'items': [item('business')]},
        {'title': point(0, 2), 'items': [item('price')]},
        {'title': point(0, 1), 'items': [item('extra', entry=0), item('technical', entry=1)]},
    ]
    return documents, raw


def test_host_uses_source_group_order_and_preserves_cover_and_internal_chapter_order():
    documents, raw = grouped_plan()
    before = deepcopy(raw)
    compiled = compile_directory(raw, documents)
    assert [chapter['id'] for chapter in compiled['chapters']] == [
        'cover', 'business', 'extra', 'technical', 'price']
    outline = outline_from_compiled({'schema_version': 2, 'run_id': 'groups', 'source_version': 1,
                                     'domain_revision': 1}, compiled)
    assert [chapter['title'] for chapter in outline['chapters']] == [
        '投标封面', '商务投标文件', '技术投标文件', '报价文件']
    assert outline['chapters'][0]['presentation_role'] == 'cover'
    assert [chapter['title'] for chapter in outline['chapters'][2]['children']] == ['技术补充', '技术原式']
    assert raw == before


def test_untitled_scope_is_a_barrier_that_source_group_order_cannot_cross():
    documents, raw = grouped_plan()
    raw['source_only'] = []
    raw['scopes'][1:3] = list(reversed(raw['scopes'][1:3]))
    raw['scopes'].insert(3, {'title': None, 'items': [item('plain')]})
    compiled = compile_directory(raw, documents)
    assert [chapter['id'] for chapter in compiled['chapters']] == [
        'cover', 'business', 'price', 'plain', 'extra', 'technical']


def test_a_cover_scope_with_a_source_title_remains_a_barrier():
    documents, raw = grouped_plan()
    documents[0]['pages'][0] += '\n投标封面组'
    raw['scopes'][0]['title'] = point(0, 3)
    compiled = compile_directory(raw, documents)
    assert [chapter['id'] for chapter in compiled['chapters']] == [
        'cover', 'business', 'extra', 'technical', 'price']


def test_explicit_priority_basis_retains_adopted_group_order():
    documents, raw = grouped_plan()
    raw['composition']['priority_basis'] = [window(0, 2)]
    compiled = compile_directory(raw, documents)
    assert [chapter['id'] for chapter in compiled['chapters']] == [
        'cover', 'business', 'price', 'extra', 'technical']


def test_group_titles_from_different_documents_are_not_sorted_by_uuid_or_source_page():
    documents, raw = grouped_plan()
    documents.append({'document_id': 'a-attachment', 'role': 'attachment', 'pages': ['报价分册']})
    raw['scopes'][2]['title'] = point(0, 0, document_id='a-attachment')
    compiled = compile_directory(raw, documents)
    assert [chapter['id'] for chapter in compiled['chapters']] == [
        'cover', 'business', 'price', 'extra', 'technical']
