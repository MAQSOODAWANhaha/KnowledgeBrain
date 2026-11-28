"""Mandatory original forms in a source book explicitly named by a submission clause."""

import re

from docreader.tender.bid_format_outline import _page_edges
from docreader.tender.directory_compile import _FORMAT_TOC, compile_directory
from docreader.tender.item_placement import (
    _NUMBERED_ANNEX, _excluded_form_headings, _grid_parts_by_line, _partition_form_windows,
)


_MUST_FILL = re.compile(r'(?:^|[。；;：:])(?:投标人|供应商|响应人)(?:必须|须|应当|应)填写(?:本表|此表|下表)[。；;，,]')
_OPTIONAL = re.compile(r'[（(](?:如有|选填|可选|仅.+适用)[）)]')


def required_book_forms(document, books, chapters):
    """A numbered parent may explicitly require its own grid or several numbered child grids."""
    doc_id = document['document_id']
    pages = document['pages']
    grids = _grid_parts_by_line([document])
    occupied = {(line['document_id'], line['page'], line['line']) for c in chapters for line in c['lines']}
    excluded = _excluded_form_headings([document])
    edges = _page_edges([document])
    nodes, issues = [], []
    for page in books:
        lines = pages[page].splitlines()
        headings = [(line, mark) for line, text in enumerate(lines)
                    if not _FORMAT_TOC.search(text) and (mark := _NUMBERED_ANNEX.match(text.strip()))]
        for idx, (start, mark) in enumerate(headings):
            if mark[3] or (doc_id, page, start) in excluded or _OPTIONAL.search(lines[start]):
                continue
            children = []
            end = len(lines)
            for line, following in headings[idx + 1:]:
                if not following[3] or following.groups()[:2] != mark.groups()[:2]:
                    end = line
                    break
                children.append((line, following))
            first_grid = next((line for line in range(start + 1, end) if (doc_id, page, line) in grids), end)
            preamble = ''.join(''.join(lines[start + 1:min(first_grid, children[0][0] if children else end)]).split())
            if not _MUST_FILL.search(preamble):
                continue
            sections = children or [(start, mark)]
            for pos, (line, number) in enumerate(sections):
                if (doc_id, page, line) in excluded or _OPTIONAL.search(lines[line]):
                    continue
                stop = sections[pos + 1][0] if pos + 1 < len(sections) else end
                owned = {(doc_id, page, i) for i in range(line, stop) if (doc_id, page, i) in grids}
                if owned and owned <= occupied:
                    continue
                name = lines[line].strip()[number.end():].strip(' ：:') or lines[line].strip()
                if not owned or occupied & owned:
                    issues.append({'code': 'outline_directory_table_unbound',
                                   'detail': f'文档 {doc_id} 第{page + 1}页必填附表「{name}」的原网格待核。'})
                    continue
                # Preserve a common instruction once, including its source fields, before the first child.
                first = start if pos == 0 and not any((doc_id, page, i) in occupied for i in range(start, line)) else line
                windows = _partition_form_windows(doc_id, pages, page, first, page, existing_lines=occupied,
                                                  format_pages=set(), next_point=(page, stop), page_edges=edges)
                copied = {(doc_id, page, i) for window in windows
                          for i in range(window['start_line'], window['end_line'] + 1)}
                if not owned <= copied:
                    issues.append({'code': 'outline_directory_table_unbound',
                                   'detail': f'文档 {doc_id} 第{page + 1}页必填附表「{name}」与已有原式交叠，归属待核。'})
                    continue
                nodes.append({'id': f'book-form-{page}-{line}', 'name': name, 'response_kind': 'form',
                              'heading': {'document_id': doc_id, 'page': page, 'line': line}, 'body_windows': windows})
                occupied.update(copied)
    if not nodes:
        return [], issues
    items = [{'node_id': node['id'], 'depth': 'self', 'presentation': 'section', 'clause': []} for node in nodes]
    compiled = compile_directory({'nodes': nodes, 'scopes': [{'items': items}]}, [document])
    return compiled['chapters'], [*issues, *compiled['issues']]
