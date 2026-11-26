"""The model selects bid-format chapters. The host copies the tender lines."""

import re
from typing import Annotated

from pydantic import Field, ValidationError

from docreader.llm.request import ModelBudgetError, ModelContractError, ModelWorkUnitExhausted
from docreader.tender.directory_compile import _FORMAT_ITEM, _FORMAT_TOC, _ROLE_LABEL, bid_format_sections
from docreader.tender.directory_sources import SourceRefs
from docreader.tender.schema import Contract, NonnegativeInt, Text
from docreader.tender.status import AuthoringTask


BID_FORMAT_OUTLINE_CONTRACT = 'bid_format_source_outline_v1'
BID_FORMAT_OPERATION = 'select_bid_format_outline_v1'
HeadingRefs = Annotated[list[NonnegativeInt], Field(min_length=1, max_length=4)]
BodyRange = Annotated[list[NonnegativeInt], Field(min_length=2, max_length=2)]
_COVER_TITLES = {'投标文件', '投标文件封面', '封面'}
_ROLE_NAMES = {'main': '主件', 'attachment': '附件', **_ROLE_LABEL}
_SUBFORM_NUMBER = re.compile(r'^(?P<number>[0-9０-９]+(?:\s*[-－]\s*[0-9０-９]+)+)\s*(?P<name>[^0-9０-９\s－-].*)$')
_NOT_APPLICABLE = re.compile(r'[（(]\s*(?:本次|本项目)?\s*不适用\s*[）)]\s*[；;]?$')


class BidFormatChapter(Contract):
    heading_refs: HeadingRefs
    name: Text
    label: Text | None = None
    parent_heading_ref: NonnegativeInt | None = None
    body: BodyRange | None = None


class BidFormatOutline(Contract):
    chapters: list[BidFormatChapter] = Field(default_factory=list)


def _issue(detail, path=()):
    return {'path': list(path), 'code': 'bid_format_outline_invalid', 'detail': detail}


def _compact(text):
    return ''.join(str(text).split())


def _annex_code(name):
    compact = _compact(name)
    return compact.startswith('附件') or compact.startswith('附录') or compact.startswith('附表')


class _Prepared:
    """BUG-332: every registered document may hold a bid-format section; each is asked and bounded on its own."""

    def __init__(self, documents, identity):
        found = bid_format_sections(documents)
        self.found = bool(found)
        self.refs = SourceRefs(documents, identity)
        self.visible = set()
        self.page_edges = set()
        self.grid_rows = {}
        self.order, self.sections = [], []
        if not found:
            return
        self.grid_rows = _grid_owners(documents, self.refs)
        self.page_edges = {self.refs.by_point[point] for point in _page_edges(documents) if point in self.refs.by_point}
        self.page_edges.difference_update(self.grid_rows)
        for role, anchor, section in found:
            points = [(anchor[0], anchor[1], anchor[2])]
            points.extend((row[0], row[1], row[2]) for row in section)
            order = [self.refs.by_point[point] for point in points]
            self.visible.update(order)
            self.order.extend(order)
            self.sections.append({'role': role, 'first': order[0], 'last': order[-1], 'payload': {
                'operation': BID_FORMAT_OPERATION, 'source_ref_format': 'host_bound_line_refs_v1',
                'lines': _heading_candidates(section, self.refs), 'section_refs': [order[0], order[-1]]}})

    def section(self, reference):
        return next(section for section in self.sections if section['first'] <= reference <= section['last'])

    def end(self, reference):
        return self.section(reference)['last']


def _heading_candidates(section, refs):
    """Short title lines and the lines that follow a bare annex code. Not the form body."""
    shown, pending = [], 0
    for row in section:
        text = row[4]
        if not text:
            continue
        ref = refs.by_point[(row[0], row[1], row[2])]
        if pending:
            shown.append([ref, row[3]])
            pending -= 1
            continue
        if _bare_annex(text):
            shown.append([ref, row[3]])
            pending = 2
            continue
        if len(text) <= 48 or text.startswith('附件') or text[:2] in '一二三四五六七八九十':
            shown.append([ref, row[3]])
    return shown


def _page_edges(documents):
    """BUG-302: repeated running lines and bare page numbers are page edges, not form text."""
    from collections import Counter

    points = set()
    for document in documents:
        pages = document['pages']
        for page, flags in enumerate(document.get('layout_decoration') or []):
            points.update((document['document_id'], page, line) for line, flag in enumerate(flags) if flag)
        numbers = []
        for page, text in enumerate(pages):
            lines = text.splitlines()
            filled = [index for index, line in enumerate(lines) if line.strip()]
            for index in {filled[0], filled[-1]} if filled else ():
                if re.fullmatch(r'\d{1,4}', lines[index].strip()):
                    numbers.append((page, index, int(lines[index].strip()) - page))
        offsets = Counter(offset for _page, _line, offset in numbers)
        texted = sum(1 for text in pages if text.strip())
        if offsets:
            offset, count = offsets.most_common(1)[0]
            if count >= max(2, int(texted * 0.6)):
                points.update((document['document_id'], page, line) for page, line, value in numbers if value == offset)
    return points


def _grid_owners(documents, refs):
    owners = {}
    for document in documents:
        identity = document['document_id']
        parts = document.get('template_parts') or {}
        for page, units in enumerate(document.get('line_units') or []):
            for line, unit_id in enumerate(units or []):
                reference = refs.by_point.get((identity, page, line))
                if unit_id in parts and reference is not None:
                    owners[reference] = (identity, unit_id)
    return owners


def _cover_window(prepared, chapters):
    """Require an independent prefix page with a source-bound cover grid."""
    limit = min((start if start is not None else heads[0]
                 for heads, _name, start, _end in chapters
                 if all(_compact(prepared.refs.text(ref)) not in _COVER_TITLES for ref in heads)),
                default=prepared.end(prepared.order[0]) + 1)
    limit = min(limit, prepared.end(prepared.order[0]) + 1)
    main = prepared.refs.point(prepared.order[0])['document_id']
    other_page = prepared.refs.point(limit)['page'] if limit <= prepared.end(prepared.order[0]) else None
    pages = {}
    for ref in prepared.order[1:]:
        point = prepared.refs.point(ref)
        if ref >= limit or point['document_id'] != main or point['page'] == other_page:
            break
        pages.setdefault(point['page'], []).append(ref)
    grid_rows = {}
    for ref, identity in prepared.grid_rows.items():
        grid_rows.setdefault(identity, []).append(ref)
    pending = False
    for refs in pages.values():
        headings = [ref for ref in refs if ref not in prepared.page_edges and ref not in prepared.grid_rows
                    and _compact(prepared.refs.text(ref)) in _COVER_TITLES]
        if not headings:
            continue
        pending = True
        for identity in {prepared.grid_rows[ref] for ref in refs if ref in prepared.grid_rows}:
            rows = grid_rows[identity]
            if not set(rows) <= set(refs):
                continue
            fields = _compact(''.join(prepared.refs.text(ref) for ref in rows))
            if ('编号' in fields and ('投标人名称' in fields or '供应商名称' in fields)
                    and any(field in fields for field in ('盖章', '签章', '日期', '年月日'))):
                return (headings[0], refs[0], refs[-1]), False
    return None, pending


def _non_applicable_spans(prepared):
    """A numbered format marked not applicable owns its subtree through the next peer."""
    headings = []
    for reference in prepared.order[1:]:
        if reference in prepared.page_edges or reference in prepared.grid_rows:
            continue
        text = prepared.refs.text(reference).strip()
        if len(text) > 48 or _FORMAT_TOC.search(text) or any(mark in text for mark in '。；，：“”'):
            continue
        subform = _SUBFORM_NUMBER.match(text)
        base = _FORMAT_ITEM.match(text)
        if subform:
            depth, name = len(re.split('[-－]', subform['number'])), subform['name']
        elif base and (name := text[base.end():].strip()):
            depth = 1
        else:
            continue
        headings.append((reference, depth, bool(_NOT_APPLICABLE.search(name))))
    return [(reference, next((ref for ref, level, _ in headings[index + 1:]
                              if level <= depth and ref <= prepared.end(reference)), prepared.end(reference) + 1))
            for index, (reference, depth, excluded) in enumerate(headings) if excluded]


def _copy_windows(prepared, start, end):
    """The form's own lines from start to end; page edges are skipped, nothing else."""
    windows = []
    for reference in range(start, end + 1):
        if reference in prepared.page_edges:
            continue
        point = prepared.refs.point(reference)
        document_id, page, line = point['document_id'], point['page'], point['line']
        if windows and windows[-1]['document_id'] == document_id and windows[-1]['page'] == page \
                and windows[-1]['end_line'] + 1 == line:
            windows[-1]['end_line'] = line
        else:
            windows.append({'document_id': document_id, 'page': page, 'start_line': line, 'end_line': line})
    return windows


def _bare_annex(text):
    from docreader.tender.directory_compile import _ANNEX_ONLY
    return _ANNEX_ONLY.match(text) is not None


def _validate_label(label, text, path):
    if label is None:
        return None
    cleaned = label.strip()
    if not cleaned:
        return None
    if _annex_code(cleaned) or _compact(cleaned) not in _compact(text):
        raise ModelContractError([_issue('章节展示标题 label 必须是所引原行的连续片段且不含附件编号', [*path, 'label'])])
    return cleaned


def decode_bid_format_outline(raw, prepared, *, allowed_parent_refs=None):
    """Accept only source fragments. Reject invented names, bad labels, or invalid parent refs."""
    if not prepared.found:
        raise ModelContractError([_issue('the tender has no bid-format section')])
    try:
        plan = BidFormatOutline.model_validate(raw).model_dump()
    except ValidationError as error:
        raise ModelContractError([{'path': list(item['loc']), 'code': item['type'], 'detail': item['msg']}
                                  for item in error.errors(include_input=False)]) from None
    if not plan['chapters']:
        raise ModelContractError([_issue('select the bid forms named in the tender')])
    skipped = _non_applicable_spans(prepared)
    chapters = []
    used, seen_heads, shared_spans = set(), {}, {}
    for index, chapter in enumerate(plan['chapters']):
        path = ['chapters', index]
        heads = chapter['heading_refs']
        text = ''.join(prepared.refs.text(reference) for reference in heads) if all(
            reference in prepared.visible for reference in heads) else ''
        name = chapter['name'].strip()
        # BUG-301: a顿号 line that names several files yields one chapter per file on the same line.
        shared = tuple(heads) in seen_heads and '、' in text and _compact(name) not in seen_heads[tuple(heads)]
        if heads != sorted(heads) or any(reference not in prepared.visible for reference in heads) or (
                any(reference in key for key in seen_heads for reference in heads) and not shared):
            raise ModelContractError([_issue('heading refs must be unused section lines in order', [*path, 'heading_refs'])])
        if any(heads[offset] + 1 != heads[offset + 1] for offset in range(len(heads) - 1)):
            raise ModelContractError([_issue('a wrapped title must be consecutive source lines', [*path, 'heading_refs'])])
        if any(start <= head < end for head in heads for start, end in skipped):
            raise ModelContractError([_issue('标题属于原文明示不适用的式样', [*path, 'heading_refs'])])
        seen_heads.setdefault(tuple(heads), set()).add(_compact(name))
        if _annex_code(name) or _compact(name) not in _compact(text):
            raise ModelContractError([_issue('章节名称必须是标题原行里的文件名，不能是附件编号或新写的名称', [*path, 'name'])])
        label = _validate_label(chapter.get('label'), text, path)
        parent_ref = chapter.get('parent_heading_ref')
        if parent_ref is not None and parent_ref >= heads[0]:
            raise ModelContractError([_issue('上级章行号 parent_heading_ref 必须位于当前章之前', [*path, 'parent_heading_ref'])])
        if chapter['body'] is None:
            chapters.append((heads, name, label, parent_ref, None, None))
            continue
        start, end = chapter['body']
        if end < start or end > prepared.end(start) or any(reference not in prepared.visible
                                                             for reference in range(start, end + 1)):
            raise ModelContractError([_issue('body must stay inside the bid-format section', [*path, 'body'])])
        if any(start < stop and skip <= end for skip, stop in skipped):
            raise ModelContractError([_issue('原式正文不能跨入不适用的式样', [*path, 'body'])])
        if not set(heads) <= set(range(start, end + 1)):
            raise ModelContractError([_issue('the file name line must sit inside its template', [*path, 'body'])])
        span = set(range(start, end + 1))
        if shared and shared_spans.get(tuple(heads)) == (start, end):
            chapters.append((heads, name, label, parent_ref, start, end))
            continue
        if span & used:
            raise ModelContractError([_issue('each tender line belongs to one form', [*path, 'body'])])
        used.update(span)
        shared_spans[tuple(heads)] = (start, end)
        chapters.append((heads, name, label, parent_ref, start, end))
    chapters.sort(key=lambda item: item[0][0])
    cover, cover_pending = _cover_window(prepared, [(h, n, s, e) for h, n, _l, _p, s, e in chapters])
    if cover:
        chapters = [chapter for chapter in chapters if cover[0] not in chapter[0]]
    # Validate parent_heading_ref references exist in the same document (or slice context) and are non-cover.
    known_heads: dict[int, str] = {}
    for heads, _name, _label, _parent_ref, _s, _e in chapters:
        known_heads.setdefault(heads[0], prepared.refs.point(heads[0])['document_id'])
    extra_heads = {
        ref: prepared.refs.point(ref)['document_id']
        for ref in (allowed_parent_refs or ())
        if ref in prepared.visible
    }
    valid_parents = {**extra_heads, **known_heads}
    for index, (heads, _name, _label, parent_ref, _s, _e) in enumerate(chapters):
        if parent_ref is None:
            continue
        if cover and parent_ref == cover[0]:
            raise ModelContractError([_issue('封面不能作为上级章', ['chapters', index, 'parent_heading_ref'])])
        doc_id = prepared.refs.point(heads[0])['document_id']
        if parent_ref not in valid_parents or valid_parents[parent_ref] != doc_id:
            raise ModelContractError([_issue('上级章行号 parent_heading_ref 必须指向同文档中先出现的已选章节',
                                             ['chapters', index, 'parent_heading_ref'])])
    if any(item[4] is None for item in chapters):
        if any(item[4] is not None for item in chapters):
            raise ModelContractError([_issue('either every chapter names its body range, or none do')])
        for index, (heads, name, label, parent_ref, _start, _end) in enumerate(chapters):
            start = heads[0]
            # Chapters sharing one listed line copy the same template up to the next other heading.
            following = next((item[0][0] for item in chapters[index + 1:]
                              if item[0][0] != start and item[0][0] <= prepared.end(start)), None)
            cutoff = next((boundary for boundary, _ in skipped if start < boundary <= prepared.end(start)),
                          prepared.end(start) + 1)
            end = min(following, cutoff) - 1 if following is not None else cutoff - 1
            if end < start:
                raise ModelContractError([_issue('chapter order leaves an empty template', ['chapters', index])])
            chapters[index] = (heads, name, label, parent_ref, start, end)
    if cover:
        heading, start, end = cover
        chapters.insert(0, ([heading], prepared.refs.text(heading).strip(), None, None, start, end))
    nodes, entries, adopted = [], [], []
    copied = set()
    ref_to_id: dict[int, str] = {}
    for number, (heads, name, label, parent_ref, start, end) in enumerate(chapters, start=1):
        heading = prepared.refs.point(heads[0])
        identity = f'format-{number}'
        # One tender line has one owner; later files listed on it keep their own chapter without a copy.
        windows = [] if tuple(heads) in copied else _copy_windows(prepared, start, end)
        copied.add(tuple(heads))
        is_cover = bool(cover and heads == [cover[0]])
        parent_id = None if is_cover or parent_ref is None else ref_to_id.get(parent_ref)
        ref_to_id.setdefault(heads[0], identity)
        node = {'id': identity, 'heading': heading, 'parent_id': parent_id, 'response_kind': 'form',
                'name': name if is_cover else (label or name), 'body_windows': windows}
        entry = {'name': heading}
        if len(heads) > 1:
            # BUG-298: the selected wrapped title lines, not a parenthesis guess.
            node['heading_lines'] = entry['heading_lines'] = len(heads)
        nodes.append(node)
        entries.append(entry)
        presentation = 'cover' if is_cover else 'section'
        adopted.append({'node_id': identity, 'entry': number - 1, 'depth': 'self',
                        'presentation': presentation, 'clause': []})
    first = nodes[0]['heading']
    result = {'nodes': nodes,
              'composition': {'priority_basis': [{'document_id': first['document_id'], 'page': first['page'],
                                                   'start_line': first['line'], 'end_line': first['line']}],
                              'entries': entries},
              'scopes': [{'items': adopted}], 'excluded': [], 'source_only': []}
    issues = _source_issues(prepared, [(h, n, s, e) for h, n, _l, _p, s, e in chapters])
    if cover_pending:
        issues.insert(0, {'code': 'outline_directory_literal_pending',
                          'detail': '格式章节中的封面标题缺少可核对的同页字段网格，原式待核。'})
    if issues:
        result['skipped_item_issues'] = issues
    return result


def _place(prepared, reference):
    point = prepared.refs.point(reference)
    role = prepared.section(reference)['role']
    return f"{_ROLE_NAMES.get(role, role)}（文档 {point['document_id']}）第 {point['page'] + 1} 页"


def _source_issues(prepared, chapters):
    """BUG-332: name every addendum form's source; keep same-name forms of different documents for the author."""
    issues, named = [], {}
    for heads, name, _start, _end in chapters:
        if prepared.section(heads[0])['role'] in _ROLE_LABEL:
            issues.append({'code': 'outline_directory_format_source',
                           'detail': f'「{name}」的原式来自{_place(prepared, heads[0])}的投标文件格式。'})
        named.setdefault(_compact(name), []).append((name, heads[0]))
    for found in named.values():
        documents = {prepared.refs.point(reference)['document_id'] for _name, reference in found}
        if len(documents) > 1:
            issues.append({'code': 'outline_directory_format_conflict',
                           'detail': f'「{found[0][0]}」同时出现在' + '、'.join(_place(prepared, reference) for _name, reference in found)
                           + '，各章均保留，请核定以哪份为准。'})
    return issues


async def select_bid_format_plan(documents, budget, execute, identity):
    """One request per document's bid-format section; an oversized one is sent in slices (BUG-331, BUG-332).

    A single section that fits is the unchanged whole-section request. Otherwise each unit returns only its own
    headings without body, the host merges every unit and decodes once; a failed unit stays pending.
    """
    import asyncio

    from docreader.tender.authoring_pipeline import _ask, _fits, _plan

    prepared = _Prepared(documents, identity)
    if not prepared.found:
        return None
    task = AuthoringTask.DIRECTORY_PLAN

    def accept(raw):
        return decode_bid_format_outline(raw, prepared)

    def units():
        found = []
        for section in prepared.sections:
            slices = _slices(section['payload'], lambda payload: _fits(budget, task, payload))
            found.extend([section['payload']] if slices is None else slices)
        return found

    payloads = await asyncio.to_thread(units)
    if len(prepared.sections) == 1 and payloads == [prepared.sections[0]['payload']]:
        await _plan(execute, task, 1, 'bid-format-plan')
        return await _ask(execute, budget, task, payloads[0], accept)
    await _plan(execute, task, len(payloads), 'bid-format-plan-units')

    async def one(payload):
        if payload is None:
            return None
        shown = {line[0] for line in payload['lines']}
        context_refs = {line[0] for line in payload.get('parent_context') or ()}

        def accept_unit(raw):
            try:
                plan = BidFormatOutline.model_validate(raw).model_dump()
            except ValidationError:
                plan = None
            if plan is None or any(chapter['body'] is not None or not set(chapter['heading_refs']) <= shown
                                   for chapter in plan['chapters']):
                raise ModelContractError([_issue('return only heading refs from these lines, without body')])
            if plan['chapters']:
                decode_bid_format_outline(raw, prepared, allowed_parent_refs=context_refs)
            return plan['chapters']
        try:
            return await _ask(execute, budget, task, payload, accept_unit)
        except (ModelWorkUnitExhausted, ModelBudgetError):
            return None

    results = await asyncio.gather(*(one(payload) for payload in payloads))
    if all(result is None for result in results):
        raise ModelWorkUnitExhausted(task)
    merged = [dict(chapter) for chapters in results if chapters for chapter in chapters]
    if any(result is None for result in results):
        # If an earlier slice failed, drop dangling cross-slice parent_heading_ref so surviving slices still compile.
        surviving = {chapter['heading_refs'][0] for chapter in merged}
        for chapter in merged:
            if chapter.get('parent_heading_ref') is not None and chapter['parent_heading_ref'] not in surviving:
                chapter['parent_heading_ref'] = None
    result = decode_bid_format_outline({'chapters': merged}, prepared)
    if any(result is None for result in results):
        result.setdefault('skipped_item_issues', []).append({
            'code': 'outline_directory_plan_failed',
            'detail': '投标文件格式章节有请求未完成（超出单次请求预算或未被采纳），其中的格式标题待核。'})
    return result


def _slices(payload, fits):
    """None when the whole title list fits; else consecutive slices, a line too long alone becomes None."""
    if fits(payload):
        return None
    lines, slices, start = payload['lines'], [], 0

    def part(end, context=()):
        if context:
            candidate = {**payload, 'lines': lines[start:end], 'parent_context': list(context)}
            if fits(candidate):
                return candidate
        return {**payload, 'lines': lines[start:end]}

    while start < len(lines):
        prior = [
            line for line in lines[:start]
            if len(line[1].strip()) <= 80 and _FORMAT_ITEM.match(line[1].strip())
        ][-4:]
        if not fits(part(start + 1)):
            slices.append(None)
            start += 1
            continue
        low, high = start + 1, len(lines)
        while low < high:
            middle = (low + high + 1) // 2
            if fits(part(middle, prior)):
                low = middle
            else:
                high = middle - 1
        end = low
        if end < len(lines):
            # Cut before the last numbered format heading so one form's wrapped lines stay together.
            cut = max((index for index in range(start + 1, end) if _FORMAT_ITEM.match(lines[index][1].strip())
                       or _SUBFORM_NUMBER.match(lines[index][1].strip()) or _bare_annex(lines[index][1].strip())),
                      default=end)
            end = cut
        slices.append(part(end, prior))
        start = end
    return slices


def compile_bid_format_plan(plan, documents):
    """BUG-332: a compile list stays inside one document, so a plan spanning documents compiles per document.

    Parts keep registered document order; host issues repeated by every part are kept once, and a revision
    document that one part cites is not reported unreviewed by another.
    """
    import json

    from docreader.tender.directory_compile import compile_plans

    order = [document['document_id'] for document in documents]
    entries = plan['composition']['entries']
    cited = sorted(
        {entry['name']['document_id'] for entry in entries}
        | {node['heading']['document_id'] for node in plan['nodes']},
        key=order.index,
    )
    if len(cited) <= 1:
        return compile_plans([plan], documents)
    compiled, seen = {'chapters': [], 'issues': []}, set()
    for index, document_id in enumerate(cited):
        own = [number for number, entry in enumerate(entries) if entry['name']['document_id'] == document_id]
        renumber = {number: position for position, number in enumerate(own)}
        own_nodes = [node for node in plan['nodes'] if node['heading']['document_id'] == document_id]
        own_node_ids = {node['id'] for node in own_nodes}
        first = entries[own[0]]['name'] if own else own_nodes[0]['heading']
        part = {'nodes': own_nodes,
                'composition': {'priority_basis': [{'document_id': document_id, 'page': first['page'],
                                                    'start_line': first['line'], 'end_line': first['line']}],
                                'entries': [entries[number] for number in own]},
                'scopes': [{'items': [{**item, 'entry': renumber[item['entry']] if item.get('entry') in renumber else None}
                                      for item in plan['scopes'][0]['items']
                                      if item.get('entry') in renumber or (item.get('entry') is None and item.get('node_id') in own_node_ids)]}],
                'excluded': [], 'source_only': []}
        if index == 0 and plan.get('skipped_item_issues'):
            part['skipped_item_issues'] = plan['skipped_item_issues']
        value = compile_plans([part], documents)
        compiled['chapters'].extend(value['chapters'])
        for issue in value['issues']:
            key = json.dumps(issue, ensure_ascii=False, sort_keys=True)
            if key in seen or (issue['code'] == 'outline_directory_revision_unreviewed'
                               and any(issue['detail'].startswith(f'{cited_id}是') for cited_id in cited)):
                continue
            seen.add(key)
            compiled['issues'].append(issue)
    return compiled
