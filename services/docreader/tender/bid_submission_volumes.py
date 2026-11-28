"""Bind the adopted format chapters to independently submitted files named in the tender."""

import re

from docreader.tender.directory_compile import (
    _FORMAT_TOC, _VOLUME_CODE, _VOLUME_ITEM, _digits, _label_key, _line_code, _volume_name,
    bid_format_sections, compile_directory,
)


_CLAUSE = re.compile(r'^\d+(?:\.\d+)+\s*[^\d.]')
_FILE_HEADING = re.compile(r'^\d+(?:\.\d+)+\s*([\u4e00-\u9fff]{2,18}(?:文件|标书|分册))\s*[：:]')
_BOOK = re.compile(r'^第[一二三四五六七八九十0-9]+卷\s*\S{1,24}$')
_SEPARATE = re.compile(r'(?:分别|单独|独立|分册)(?:进行|编制并)?(?:上传|递交|提交|编制|装订|封装)')
_DESTINATION = re.compile(r'(?:上传|递交|提交)(?:到|至|于)')
_NEGATIVE = re.compile(r'(?:不得|不可|禁止|不必|无需|不要求|不应|可以|允许|可)(?:再|进行)?(?:分别|单独|独立|分册|上传|递交|提交)')


def _source_groups(document):
    groups, current, member = [], None, None
    for page, text in enumerate(document['pages']):
        for line, raw in enumerate(text.splitlines()):
            row = raw.strip()
            if not row or _FORMAT_TOC.search(row):
                continue
            named_file = _FILE_HEADING.match(row)
            name = named_file[1] if named_file else _volume_name(row)
            if name:
                current = {'name': name, 'heading': {'document_id': document['document_id'], 'page': page, 'line': line},
                           'end_line': line, 'clause': row, 'members': []}
                groups.append(current)
                member = None
            elif _CLAUSE.match(row):
                current = member = None
            elif current:
                if _VOLUME_ITEM.match(row):
                    current['members'].append(row)
                    member = len(current['members']) - 1
                elif member is not None and not current['members'][member].endswith(('。', '；', ';')):
                    current['members'][member] += row
                elif member is None and page == current['heading']['page'] and not current['clause'].endswith(
                        ('。', '：', ':', '；', ';')):
                    current['clause'] += row
                    current['end_line'] = line
    return groups


def _separate_instructions(names, pages):
    submitted, disputed = set(), set()
    for page in pages:
        for clause in re.split(r'[。；;①②③④⑤⑥]', ''.join(page.split())):
            mentioned = {name for name in names if name in clause}
            if _NEGATIVE.search(clause):
                disputed.update(mentioned)
            elif _SEPARATE.search(clause) or (len(mentioned) == 1 and _DESTINATION.search(clause)):
                submitted.update(mentioned)
    return submitted, disputed


def _source_locations(document, groups, headings):
    """The annex containing a format, or a technical book explicitly cited by a submission clause."""
    annexes, books = {}, {}
    for _role, _anchor, section in bid_format_sections([document]):
        annex = None
        for _, page, line, _, text in section:
            if code := _line_code(text):
                annex = code
            if (page, line) in headings:
                annexes[page, line] = annex
    owners = []
    for page, text in enumerate(document['pages']):
        for line in text.splitlines():
            title = ''.join(line.split())
            if _BOOK.fullmatch(title) and not _FORMAT_TOC.search(title):
                owners = [g['name'] for g in groups if title in ''.join(g['clause'].split())]
        if len(owners) == 1:
            books[page] = owners[0]
    return annexes, books


def _membership(chapter, groups, annex, book):
    title = ''.join(_label_key(chapter['title']).split())
    matches = {}
    for group in groups:
        for rank, member in enumerate(group['members']):
            codes = {(_digits(match[1]), match[2].upper()) for match in _VOLUME_CODE.finditer(member)}
            if ((annex and (annex in codes or (annex[0], '') in codes))
                    or (len(title) >= 2 and title in ''.join(member.split()))):
                matches.setdefault(group['name'], rank)
    if len(matches) > 1:
        return None, float('inf')
    if matches:
        return next(iter(matches.items()))
    if not matches and book:
        return book, float('inf')
    return None


def _containers(document, groups):
    nodes, scopes = [], []
    for index, group in enumerate(groups):
        heading = group['heading']
        window = {'document_id': heading['document_id'], 'page': heading['page'],
                  'start_line': heading['line'], 'end_line': group['end_line']}
        identity = f'submission-volume-{index}'
        nodes.append({'id': identity, 'name': group['name'], 'heading': heading,
                      'response_kind': 'narrative', 'body_windows': []})
        scopes.append({'title': heading, 'items': [{'node_id': identity, 'depth': 'self',
                       'presentation': 'section', 'clause': [window]}]})
    containers = compile_directory({'nodes': nodes, 'scopes': scopes}, [document])['chapters']
    for c in containers:
        if re.search(r'(?:须|应)(?:逐[一条项])?(?:响应|应答)', c['obligations']):
            c['requirements'] = [c['obligations']]
            c['_requirement_rows'] = [c['_source_obligation_rows']]
            c['obligations'] = ''
    return containers


def bind_submission_volumes(compiled, documents):
    chapters = compiled['chapters']
    if not chapters or any(c.get('group') for c in chapters):
        return compiled
    mains = [doc for doc in documents if doc.get('role', 'main') == 'main']
    if len(mains) != 1:
        return compiled
    document, = mains
    groups = _source_groups(document)
    names = {g['name'] for g in groups}
    submitted, disputed = _separate_instructions(names | {'商务文件', '技术文件', '报价文件'}, document['pages'])
    if not submitted:
        return compiled
    if len(submitted) < 2 or submitted != names or disputed & submitted or len(groups) != len(names):
        compiled['issues'].append({'code': 'outline_directory_applicability_unconfirmed',
                                   'detail': f'来源 {document["document_id"]} 的独立递交要求涉及'
                                             f'{"、".join(sorted(submitted))}，请核定分册及各册组成。'})
        return compiled
    headings = {(c['heading']['page'], c['heading']['line']) for c in chapters}
    annexes, books = _source_locations(document, groups, headings)
    membership = {}
    for c in chapters:
        point = c['heading']
        if point['document_id'] == document['document_id'] and c['presentation'] != 'cover':
            match = _membership(c, groups, annexes.get((point['page'], point['line'])), books.get(point['page']))
            if match:
                membership[c['id']] = match
    by_id = {c['id']: c for c in chapters}
    for c in chapters:
        parent = by_id.get(c['parent_id'])
        while parent and parent['id'] not in membership:
            parent = by_id.get(parent['parent_id'])
        if parent:
            membership.setdefault(c['id'], membership[parent['id']])
        if c['parent_id'] and membership.get(c['parent_id'], (None,))[0] != membership.get(c['id'], (None,))[0]:
            c['parent_id'] = None
    covers = [c for c in chapters if c['presentation'] == 'cover']
    pending = [c for c in chapters if c['presentation'] != 'cover' and not membership.get(c['id'], (None,))[0]]
    ordered = list(covers)
    for container in _containers(document, groups):
        members = [c for c in chapters if membership.get(c['id'], (None,))[0] == container['title']]
        for c in members:
            c.update(group=container['group'], group_heading=container['group_heading'])
        ordered.extend([container, *sorted(members, key=lambda c: membership[c['id']][1])])
    compiled['chapters'] = [*ordered, *pending]
    for c in pending:
        if c['parent_id'] is None:
            point = c['heading']
            compiled['issues'].append({'code': 'outline_directory_applicability_unconfirmed',
                                       'detail': f'「{c["title"]}」的分册归属待核，保留在整稿。'
                                                 f'来源：{point["document_id"]} 第{point["page"] + 1}页。'})
    return compiled
