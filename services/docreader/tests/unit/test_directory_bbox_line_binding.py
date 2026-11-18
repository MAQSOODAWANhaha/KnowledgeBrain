"""Native grid ownership follows exact words and real word-box overlap."""

from types import SimpleNamespace
from xml.sax.saxutils import escape

import pytest

from docreader.tender import directory_compile as compiler


def bind(monkeypatch, lines, boxes, rectangles=None):
    rectangles = rectangles or {'table': (0, 0, 40, 40)}
    words = ''.join(f'<word xMin="{left}" yMin="{top}" xMax="{right}" yMax="{bottom}">{escape(text)}</word>'
                    for text, left, top, right, bottom in boxes)
    xhtml = f'<html><body><page width="100" height="100">{words}</page></body></html>'
    calls = []

    def run(command, **_options):
        calls.append(command)
        return SimpleNamespace(returncode=0, stdout=xhtml.encode())

    monkeypatch.setattr('subprocess.run', run)
    units = [{'unit_id': identity, 'grid': {'row_count': 1},
              'locator': {'page_ordinal': 0, 'left': left, 'right': right,
                          'top': 100-top, 'bottom': 100-bottom}}
             for identity, (left, top, right, bottom) in rectangles.items()]
    owners = [[None] * len(lines)]
    compiler._bind_grid_geometry(['\n'.join(lines)], owners, units, b'%PDF synthetic bbox fixture')
    assert len(calls) == 1
    return owners[0]


def test_one_visual_line_with_different_font_centres_keeps_both_columns_in_the_native_grid(monkeypatch):
    owners = bind(monkeypatch, ['甲 乙'], [('甲', 2, 10, 12, 20), ('乙', 22, 13.241, 32, 23.241)])
    assert owners == ['table']


@pytest.mark.parametrize('lines,boxes,rectangles', [
    (['甲乙'], [('甲', 2, 10, 12, 20), ('乙', 22, 22, 32, 32)], None),
    (['甲乙'], [('甲', 2, 10, 12, 20), ('乙', 2, 18, 12, 28)], None),
    (['甲乙'], [('甲', 2, 10, 12, 20), ('乙', 22, 10, 32, 20)],
     {'left': (0, 0, 15, 40), 'right': (20, 0, 40, 40)}),
    (['甲乙', '甲乙'], [('甲', 2, 10, 12, 20), ('乙', 22, 13.241, 32, 23.241)], None),
    (['甲乙'], [('甲', 2, 10, 12, 20), ('乙', 22, 13.241, 42, 23.241)], None),
    (['甲乙'], [('甲', 2, 10, 12, 20), ('乙', 22, 13.241, 32, 23.241)],
     {'first': (0, 0, 40, 40), 'second': (0, 0, 40, 40)}),
    (['甲乙'], [('甲', 2, 2, 12, 12), ('乙', 22, 5.241, 32, 15.241),
                ('甲', 2, 22, 12, 32), ('乙', 22, 25.241, 32, 35.241)], None),
    (['甲乙丙'], [('甲', 2, 10, 12, 20), ('乙', 22, 13.241, 32, 23.241)], None),
])
def test_fallback_does_not_join_neighbouring_rows_tables_repetitions_or_outside_words(monkeypatch, lines, boxes, rectangles):
    assert bind(monkeypatch, lines, boxes, rectangles) == [None] * len(lines)


@pytest.mark.parametrize('lines,boxes,rectangles', [
    (['甲乙'], [('甲', 2, 10, 12, 20), ('乙', 22, 10, 32, 20)],
     {'first': (0, 0, 40, 40), 'second': (0, 0, 40, 40)}),
    (['甲乙'], [('甲', 2, 10, 12, 20), ('乙', 22, 10, 32, 20),
                ('甲', 2, 25, 12, 35), ('乙', 22, 25, 32, 35)],
     {'first': (0, 0, 40, 21), 'second': (0, 24, 40, 40)}),
    (['甲乙', '甲乙'], [('甲', 2, 10, 12, 20), ('乙', 22, 10, 32, 20)], None),
])
def test_existing_exact_visual_groups_do_not_guess_a_grid_or_reuse_one_physical_row(monkeypatch, lines, boxes, rectangles):
    assert bind(monkeypatch, lines, boxes, rectangles) == [None] * len(lines)


def test_touching_vertical_edges_do_not_count_as_word_overlap(monkeypatch):
    assert bind(monkeypatch, ['甲乙'], [('甲', 2, 10, 12, 20), ('乙', 22, 20, 32, 30)]) == [None]


def test_existing_exact_repeated_visual_lines_keep_their_one_to_one_source_order(monkeypatch):
    boxes = [('甲', 2, 10, 12, 20), ('乙', 22, 10, 32, 20),
             ('甲', 2, 25, 12, 35), ('乙', 22, 25, 32, 35)]
    assert bind(monkeypatch, ['甲乙', '甲乙'], boxes) == ['table', 'table']


@pytest.mark.parametrize('boxes', [
    [('甲', 2, 10, 12, 20), ('乙', 22, 10, 42, 20)],
    [('甲', 2, 10, 15, 20), ('乙', 10, 10, 25, 20)],
    [('甲', 2, 10, 12, 20), ('乙', 22, 10, 32, 20),
     ('甲', 2, 25, 12, 35), ('乙', 22, 25, 32, 35)],
])
def test_existing_exact_visual_rows_keep_font_bounds_and_same_grid_ownership_contract(monkeypatch, boxes):
    assert bind(monkeypatch, ['甲乙'], boxes) == ['table']


def test_bbox_words_retains_its_existing_public_coordinate_contract():
    text = '<page><word xMin="2" yMin="10" xMax="12" yMax="20">甲&amp;乙</word></page>'
    assert compiler._bbox_words(text) == [(7.0, 15.0, '甲&乙')]


def test_geometric_overlap_candidates_are_indexed_once_per_page(monkeypatch):
    original = getattr(compiler, '_overlap_word_lines', lambda _boxes: {})
    indexed = []

    def count(boxes):
        indexed.append(len(boxes))
        return original(boxes)

    monkeypatch.setattr(compiler, '_overlap_word_lines', count, raising=False)
    owners = bind(monkeypatch, ['未绑定文字' + str(number) for number in range(25)] + ['甲乙'],
                  [('甲', 2, 10, 12, 20), ('乙', 22, 13.241, 32, 23.241)])
    assert owners[-1] == 'table'
    assert indexed == [2]
