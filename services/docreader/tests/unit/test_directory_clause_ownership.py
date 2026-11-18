"""Explicit body ownership removes only the same target's repeated source coordinates."""

from docreader.tender.directory_compile import compile_directory
from test_directory_compile import docs, item, node, plan, window


def test_explicit_body_and_clause_keep_only_additional_source_conditions():
    pages = ['引用原式\n原表表头\n详见分项报价表\n仅代理商适用']
    compiled = compile_directory(plan([
        {**node('form', 0, 0), 'body_windows': [window(0, 0, 2)]}],
        [item('form', clause=[window(0, 1, 3)])]), docs(pages))
    chapter = compiled['chapters'][0]
    assert chapter['text'] == '引用原式\n原表表头\n详见分项报价表'
    assert chapter['obligations'] == '仅代理商适用'


def test_equal_text_from_a_different_source_coordinate_is_not_removed():
    pages = ['引用原式\n须盖章\n须盖章']
    compiled = compile_directory(plan([
        {**node('form', 0, 0), 'body_windows': [window(0, 0, 1)]}],
        [item('form', clause=[window(0, 2)])]), docs(pages))
    assert compiled['chapters'][0]['obligations'] == '须盖章'


def test_legacy_raw_clause_keeps_its_original_replay_interpretation():
    compiled = compile_directory(plan([node('form', 0, 0)],
        [item('form', clause=[window(0, 1)])]), docs(['原式\n须盖章']))
    assert compiled['chapters'][0]['obligations'] == '须盖章'
