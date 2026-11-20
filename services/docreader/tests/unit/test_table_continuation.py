"""Ruled-table and paragraph continuation stay annotations."""

from copy import deepcopy

from docreader.parser.continuation import paragraph_continuations, ruled_table_continuations
from docreader.parser.pdf_tables import continuations_for_extracted_pages, extract_tables_from_page


def _table(identity, *, bottom, top, ruled=True, split_row=False, edges=(10, 110, 210)):
    return {'table_id': identity, 'ruled': ruled, 'top': top, 'bottom': bottom,
            'column_edges': list(edges), 'column_count': len(edges) - 1, 'split_row': split_row}


def _pair():
    return [
        {'page': 0, 'height': 800, 'tables': [_table('a', bottom=40, top=220)],
         'body_spans': [{'top': 500, 'bottom': 480}]},
        {'page': 1, 'height': 800, 'tables': [_table('b', bottom=400, top=760, edges=(12, 112, 208))],
         'body_spans': [{'top': 300, 'bottom': 280}]},
    ]


def test_qualifying_ruled_tables_are_linked_and_both_grids_stay_intact():
    pages = _pair()
    original = deepcopy(pages)
    links = ruled_table_continuations(pages)
    assert pages == original
    assert links == [{
        'kind': 'ruled_table', 'before_page': 0, 'before_table_id': 'a',
        'after_page': 1, 'after_table_id': 'b', 'repeated_header_rows': 0,
    }]
    assert pages[0]['tables'][0]['column_count'] == 2
    assert pages[1]['tables'][0]['table_id'] == 'b'


def test_failing_pairs_have_no_link_and_no_merged_cells():
    def absent(mutate):
        pages = _pair()
        mutate(pages)
        before = deepcopy(pages)
        assert ruled_table_continuations(pages) == []
        assert pages == before

    absent(lambda pages: pages[1]['tables'][0].update(ruled=False))
    absent(lambda pages: pages[0].__setitem__('body_spans', [{'top': 10, 'bottom': 4}]))
    absent(lambda pages: pages[1].__setitem__('page', 2))
    absent(lambda pages: pages.insert(1, {'page': 1, 'scanned': True, 'height': 800, 'tables': [], 'body_spans': []}) or pages[2].__setitem__('page', 2))
    absent(lambda pages: pages[0]['tables'][0].update(split_row=True))


def _rules(left, right, bottom, top, columns, *, open_bottom=False):
    xs = [left + (right - left) * index / columns for index in range(columns + 1)]
    horizontal = [(left, bottom, right), (left, top, right)]
    if not open_bottom:
        horizontal.append((left, (bottom + top) / 2, right))
    vertical = []
    low = bottom - 30 if open_bottom else bottom
    for x_pos in xs:
        vertical.append((x_pos, low, top))
    return horizontal, vertical


def _chars(text, x, y):
    return [{'ch': char, 'x0': x + index * 8, 'x1': x + index * 8 + 7, 'y0': y, 'y1': y + 10} for index, char in enumerate(text)]


def test_extracted_closed_tables_link_and_an_open_bottom_does_not():
    closed_h, closed_v = _rules(10, 210, 20, 180, 2)
    later_h, later_v = _rules(12, 208, 620, 780, 2)
    header = _chars('列甲', 20, 150) + _chars('列乙', 120, 150)
    data = _chars('1', 20, 40) + _chars('2', 120, 40)
    later_header = _chars('列甲', 22, 740) + _chars('列乙', 122, 740)
    later_data = _chars('3', 22, 650) + _chars('4', 122, 650)
    pages = [
        {'page': 0, 'height': 800, 'horizontal': closed_h, 'vertical': closed_v, 'chars': header + data},
        {'page': 1, 'height': 800, 'horizontal': later_h, 'vertical': later_v, 'chars': later_header + later_data},
    ]
    extracted, links = continuations_for_extracted_pages(pages)
    assert all(grid.split_row is False and grid.ruled for page in extracted for grid in page)
    assert len(extracted[0]) == 1 and len(extracted[1]) == 1
    assert extracted[0][0].cells and extracted[1][0].cells
    assert links and links[0]['before_table_id'] != links[0]['after_table_id']
    assert links[0]['repeated_header_rows'] == 1
    open_h, open_v = _rules(10, 210, 20, 180, 2, open_bottom=True)
    opened, open_links = continuations_for_extracted_pages([
        {'page': 0, 'height': 800, 'horizontal': open_h, 'vertical': open_v, 'chars': header + data},
        pages[1],
    ])
    assert opened[0] and opened[0][0].split_row is True
    assert open_links == []
    top_h = [(12, 700, 208), (12, 780, 208)]
    top_v = [(12, 620, 790), (110, 620, 790), (208, 620, 790)]
    closed_lower_h, closed_lower_v = _rules(12, 208, 400, 560, 2)
    topped, top_links = continuations_for_extracted_pages([
        pages[0],
        {'page': 1, 'height': 800, 'horizontal': [*top_h, *closed_lower_h],
         'vertical': [*top_v, *closed_lower_v],
         'chars': later_header + later_data + _chars('列甲', 22, 520) + _chars('9', 22, 430)},
    ])
    assert any(grid.split_row for grid in topped[1])
    assert top_links == []
    assert extract_tables_from_page(type('P', (), {'get_objects': lambda self: []})(), type('R', (), {})(), []) == []


def test_paragraph_continuation_is_an_annotation_and_does_not_join_text():
    pages = [
        {'page': 0, 'lines': [{'text': '投标人应具备', 'decoration': False}, {'text': '招标文件', 'decoration': True}]},
        {'page': 1, 'lines': [{'text': '招标文件', 'decoration': True}, {'text': '相应资格条件', 'decoration': False}]},
    ]
    original = deepcopy(pages)
    links = paragraph_continuations(pages)
    assert pages == original
    assert links == [{'kind': 'paragraph', 'before_page': 0, 'before_line': 0, 'after_page': 1, 'after_line': 1}]
    assert pages[0]['lines'][0]['text'] == '投标人应具备'
    assert pages[1]['lines'][1]['text'] == '相应资格条件'

    finished = deepcopy(pages)
    finished[0]['lines'][0]['text'] = '投标人应具备。'
    assert paragraph_continuations(finished) == []
    numbered = deepcopy(pages)
    numbered[1]['lines'][1]['text'] = '1. 资格条件'
    assert paragraph_continuations(numbered) == []
