"""Obligations overlap by source coordinate, never merely by equal text."""

import pytest

from docreader.tender.directory_compile import _Source, _obligations


def source(pages, *documents):
    return _Source([{'document_id': 'tender', 'role': 'main', 'pages': pages}, *documents])


def window(start, end=None, page=0, document_id='tender'):
    return {'document_id': document_id, 'page': page, 'start_line': start,
            'end_line': start if end is None else end}


def test_multiline_clause_and_entry_emit_the_same_source_line_once():
    original = source(['  (3) 投标保证金承诺函；\n  (4) 履约保证函；'])
    actual = _obligations({'clause': [window(0, 1)]},
                          {'name': ('tender', 0, 1)}, original)
    assert actual == '(3) 投标保证金承诺函；\n  (4) 履约保证函；'


def test_overlapping_clause_windows_keep_only_new_source_lines_in_order():
    original = source(['提交承诺\n加盖公章\n附证明文件'])
    actual = _obligations({'clause': [window(0, 1), window(1, 2)]}, None, original)
    assert actual == '提交承诺\n加盖公章\n附证明文件'


def test_indented_entry_does_not_repeat_its_stripped_clause():
    original = source(['  履约保证函；'])
    actual = _obligations({'clause': [window(0)]}, {'name': ('tender', 0, 0)}, original)
    assert actual == '履约保证函；'


def test_covered_body_lines_are_not_repeated_by_clauses_or_entry():
    original = source(['已在原式\n附加义务\n适用条件'])
    covered = {('tender', 0, 0)}
    actual = _obligations({'clause': [window(0, 1), window(1, 2)]},
                          {'name': ('tender', 0, 0)}, original, covered)
    assert actual == '附加义务\n适用条件'
    assert covered == {('tender', 0, 0)}


@pytest.mark.parametrize('pages,documents,clauses', [
    (['须盖章\n须盖章'], [], [window(0), window(1)]),
    (['须盖章', '须盖章'], [], [window(0), window(0, page=1)]),
    (['须盖章'], [{'document_id': 'supplement', 'role': 'main', 'pages': ['须盖章']}],
     [window(0), window(0, document_id='supplement')]),
])
def test_equal_text_at_different_source_coordinates_is_preserved(pages, documents, clauses):
    actual = _obligations({'clause': clauses}, None, source(pages, *documents))
    assert actual == '须盖章\n须盖章'


def test_equal_entry_text_at_a_different_source_coordinate_is_preserved():
    actual = _obligations({'clause': [window(0)]}, {'name': ('tender', 0, 1)},
                          source(['须盖章\n须盖章']))
    assert actual == '须盖章\n须盖章'
