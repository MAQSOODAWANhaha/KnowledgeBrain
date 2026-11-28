"""New requirement chain: fill missing submission forms and place verified items by short ID.

Rules and five questions: business-workflow「新链：格式补位与事项归属」; fields: field dictionary.
"""

import asyncio
from copy import deepcopy
import json
import re
from typing import Annotated

from pydantic import Field, ValidationError

from docreader.llm.request import ModelBudgetError, ModelContractError, ModelWorkUnitExhausted
from docreader.tender.bid_format_outline import _NOT_APPLICABLE, _page_edges
from docreader.tender.directory_compile import _FORMAT_ITEM, _FORMAT_TOC, _LABEL_NOTE, _REVISIONS, _ROLE_LABEL, _template_identity
from docreader.tender.schema import Contract, Text
from docreader.tender.status import AuthoringTask

REQUIREMENT_CHAIN_CONTRACT = 'single_requirement_chain_v1'
PLACEMENT_OPERATION = 'place_items_v1'
EVALUATION_PENDING_DETAIL = '模型建议按评标程序处理，请按原件核定是否另设响应。'
NOT_REQUIRED = {
    'evaluation': '评标程序或评委动作，投标文件不需单独响应。',
    'performance': '签约后才发生的合同手续，投标文件不需说明。',
    'document_wide': '整份投标文件的格式、份数或递交约束。',
    'not_applicable': '原文明示本项目不适用。',
}
_SEMANTIC = ('kind', 'subject', 'scope', 'text', 'conditions', 'exceptions', 'submission', 'consequence', 'scoring')
_OPENING = 120
# Keep each batch small enough to avoid long-tail reasoning bloat while fitting in one parallel wave.
PLACEMENT_BATCH_ITEMS = 120
# Stop a batch at half the input window. Packing to the ceiling still fits, but the relay
# spends many minutes before the first token.
PLACEMENT_BATCH_INPUT_FRACTION = 0.5


class ItemPlacementResponse(Contract):
    placements: list[Annotated[list[Text | None], Field(min_length=2, max_length=2)]]


def _compact(text):
    return re.sub(r'\s+', '', text or '')


def _walk(chapters, titles=(), *, skip_covers=False):
    for chapter in chapters:
        if skip_covers and chapter.get('presentation_role') == 'cover':
            continue
        path = (*titles, chapter['title'])
        for target in chapter.get('targets', []):
            yield path, target
        yield from _walk(chapter.get('children', []), path, skip_covers=skip_covers)


def _items(context):
    """Verified ledger rows with semantic fields and frozen evidence pages."""
    pages = {}
    frozen = context.artifacts.read_json(context.input['frozen_ref'])
    for document in frozen['documents']:
        index = context.repository.source_index(document['source_index_ref'])
        for identity, record in index.by_id.items():
            locator = record.get('physical_locator') or record.get('locator') or {}
            pages[identity] = (document['document_id'], locator.get('page_ordinal'))
    rows = []
    for value in context.payload['rules']:
        body = value.get('item') or value['effective_rule']
        evidence = []
        for entry in body.get('evidence', []):
            document_id, page = pages.get(context.range_roots.get(entry['range_id'], entry['range_id']), (None, None))
            evidence.append({'document_id': document_id, 'page': page, 'quote': entry['quote']})
        rows.append({'rule_ref': context.rules[value['rule_id']],
                     'semantic': {key: deepcopy(body[key]) for key in _SEMANTIC if key in body},
                     'evidence': evidence})
    return rows


def _grid_parts_by_line(documents):
    points = {}
    for document in documents:
        identity = document['document_id']
        parts = document.get('template_parts') or {}
        for page, units in enumerate(document.get('line_units') or []):
            for line, unit_id in enumerate(units or []):
                if unit_id in parts:
                    points[identity, page, line] = parts[unit_id]
    return points


_NUMBERED_ANNEX = re.compile(
    r'^(?:技术|商务)?(附表|附件|附录)\s*([A-Za-z]|[0-9]+[A-Za-z]?)([.．－-][0-9]+(?:[.．－-][0-9]+)*)?')


def _excluded_form_headings(documents):
    """A numbered parent marked inapplicable also excludes its numbered children."""
    excluded = set()
    for document in documents:
        parent = None
        for page, content in enumerate(document['pages']):
            for line, text in enumerate(content.splitlines()):
                text = text.strip()
                if _FORMAT_TOC.search(text):
                    continue
                number = _NUMBERED_ANNEX.match(text)
                marked = _NOT_APPLICABLE.search(text)
                if number:
                    path = (number[1], *re.findall(r'[A-Za-z]+|[0-9]+', number[0][number.end(1):].upper()))
                    if parent and (path == parent or path[:len(parent)] != parent):
                        parent = None
                    if marked and not parent:
                        parent = path
                if marked or (number and parent):
                    excluded.add((document['document_id'], page, line))
    return excluded


def _existing_plan_lines(plan):
    lines, ids = set(), set()
    for node in plan.get('nodes', []):
        if isinstance(node.get('id'), str):
            ids.add(node['id'])
        heading = node.get('heading')
        if isinstance(heading, dict) and all(k in heading for k in ('document_id', 'page', 'line')):
            lines.add((heading['document_id'], heading['page'], heading['line']))
        for window in node.get('body_windows') or ():
            doc_id, page = window['document_id'], window['page']
            for line_no in range(window['start_line'], window['end_line'] + 1):
                lines.add((doc_id, page, line_no))
    return lines, ids


def _partition_form_windows(document_id, pages, start_page, start_line, last_page, *,
                            next_point, existing_lines, format_pages, page_edges):
    """Copy lines from heading through last_page without crossing boundaries or page edges."""
    windows = []
    for current in range(start_page, last_page + 1):
        if (document_id, current) in format_pages:
            break
        start = start_line if current == start_page else 0
        count = len(pages[current].splitlines())
        stopped = False
        for line_no in range(start, count):
            if next_point is not None and (current, line_no) >= next_point:
                stopped = True
                break
            if (document_id, current, line_no) in existing_lines:
                stopped = True
                break
            if (document_id, current, line_no) in page_edges and (current, line_no) != (start_page, start_line):
                continue
            if windows and windows[-1]['page'] == current and windows[-1]['end_line'] + 1 == line_no:
                windows[-1]['end_line'] = line_no
            else:
                windows.append({'document_id': document_id, 'page': current,
                                'start_line': line_no, 'end_line': line_no})
        if stopped:
            break
    return windows


def submission_form_nodes(documents, plan, items, format_pages):
    """Plan nodes for named submission forms that the bid-format chapter does not contain.

    BUG-332: an amendment or clarification that restates a form already in the outline is added too, as a conflict.
    BUG-350: candidates are sorted per document and partitioned before the next candidate, existing nodes, or format_pages.
    """
    named = {}
    for node in plan.get('nodes', []):
        raw_name = node.get('name') or ''
        named[_compact(raw_name)] = node
        stripped = _FORMAT_ITEM.sub('', raw_name.strip(), count=1).strip()
        if stripped:
            named.setdefault(_compact(stripped), node)
    existing_lines, existing_ids = _existing_plan_lines(plan)
    existing_lines.update(_excluded_form_headings(documents))
    grid_parts = _grid_parts_by_line(documents)
    page_edges = _page_edges(documents).difference(grid_parts)
    roles = {document['document_id']: document.get('role') or 'main' for document in documents}
    wanted = {}
    for row in items:
        if row['semantic'].get('kind') != 'submission_requirement':
            continue
        unconditional = not row['semantic'].get('conditions') and not row['semantic'].get('exceptions')
        for name in (row['semantic'].get('submission') or {}).get('documents') or []:
            key = _compact(name)
            if not key:
                continue
            outside = [e for e in row['evidence'] if e['page'] is not None
                       and (e['document_id'], e['page']) not in format_pages]
            conflict = named.get(key)
            if conflict is not None:
                outside = [e for e in outside if roles.get(e['document_id']) in _REVISIONS]
            for evidence in outside:
                source = evidence['document_id']
                entry = wanted.setdefault((key, source), {'name': name.strip(), 'evidence': [],
                                                          'cited_pages': set(), 'conflict': conflict})
                entry['evidence'].append(evidence)
                if unconditional and not _LABEL_NOTE.search(name) and _compact(evidence['quote']) == key:
                    entry['cited_pages'].add(evidence['page'])
    by_id = {document['document_id']: document for document in documents}
    candidates, claimed_headings = [], set()
    for (key, document_id), entry in wanted.items():
        heading = None
        document = by_id.get(document_id)
        if document is None:
            continue
        evidence_pages = sorted({e['page'] for e in entry['evidence']})
        search_pages = dict.fromkeys([*sorted(entry['cited_pages']),
                                      *(p for page in evidence_pages for p in (page - 1, page))])
        for page in search_pages:
            if not 0 <= page < len(document['pages']) or (document_id, page) in format_pages:
                continue
            for line, text in enumerate(document['pages'][page].splitlines()):
                point = (document_id, page, line)
                if _compact(text) == key and point not in existing_lines and point not in claimed_headings:
                    heading = point
                    break
            if heading:
                break
        if heading is None:
            continue
        claimed_headings.add(heading)
        candidates.append((heading, entry))
    candidates.sort(key=lambda pair: pair[0])
    nodes, seq = [], 0
    for idx, (heading, entry) in enumerate(candidates):
        document_id, page, line = heading
        pages = by_id[document_id]['pages']
        evidence_pages = [e['page'] for e in entry['evidence'] if e['document_id'] == document_id and e['page'] >= page]
        last = max(evidence_pages, default=page)
        next_point = (
            (candidates[idx + 1][0][1], candidates[idx + 1][0][2])
            if idx + 1 < len(candidates) and candidates[idx + 1][0][0] == document_id
            else None
        )
        windows = _partition_form_windows(
            document_id, pages, page, line, last,
            next_point=next_point, existing_lines=existing_lines,
            format_pages=format_pages, page_edges=page_edges,
        )
        if not windows:
            continue
        seq += 1
        while f'submission-{seq}' in existing_ids:
            seq += 1
        node_id = f'submission-{seq}'
        existing_ids.add(node_id)
        required_grids = set()
        if page in entry['cited_pages']:
            for window in windows:
                for line_no in range(window['start_line'], window['end_line'] + 1):
                    if part := grid_parts.get((document_id, window['page'], line_no)):
                        required_grids.add(_template_identity(part))
        nodes.append({'node': {'id': node_id, 'parent_id': None, 'response_kind': 'form',
                               'name': entry['name'],
                               'heading': {'document_id': document_id, 'page': page, 'line': line},
                               'body_windows': windows},
                      'point': heading, 'conflict': entry['conflict'], 'role': roles.get(document_id),
                      'required_grids': required_grids})
    return nodes


def add_submission_forms(plan, documents, items, format_pages):
    """Append supplementary forms and return their independently required source grids by name."""
    extra = submission_form_nodes(documents, plan, items, format_pages)
    if not extra:
        return plan, {}
    plan = deepcopy(plan)
    entries = plan['composition']['entries']
    items_in_scope = plan['scopes'][0]['items']
    for added in sorted(extra, key=lambda item: item['point']):
        # BUG-339: keep outside-format submissions after the bid-format chapter rather than
        # jumping ahead of the cover and first tender chapter by raw PDF page number.
        plan['nodes'].append(added['node'])
        if added['conflict'] is not None:
            position = len(entries)
            entries.insert(position, {'name': deepcopy(added['node']['heading'])})
            items_in_scope.append({'node_id': added['node']['id'], 'entry': position, 'depth': 'self',
                                   'presentation': 'section', 'clause': []})
        else:
            items_in_scope.append({'node_id': added['node']['id'], 'entry': None, 'depth': 'self',
                                   'presentation': 'section', 'clause': []})
        original = added['conflict']
        if original is not None:
            point, heading = original['heading'], added['node']['heading']
            plan.setdefault('skipped_item_issues', []).append({
                'code': 'outline_directory_format_conflict',
                'detail': f"「{added['node']['name']}」同时出现在投标文件格式（文档 {point['document_id']}）第 {point['page'] + 1} 页"
                          f"和{_ROLE_LABEL[added['role']]}（文档 {heading['document_id']}）第 {heading['page'] + 1} 页，"
                          '各章均保留，请核定以哪份为准。'})
    # Conflicts are not outside-format candidates: they stay even when no item is placed in them.
    outside = {}
    for added in extra:
        if added['conflict'] is None:
            outside.setdefault(added['node']['name'], set()).update(added['required_grids'])
    return plan, outside


def placement_positions(outline, outside_format=None):
    outside_format = outside_format or {}
    candidates = {}
    for root in outline['chapters']:
        for chapter in (root['children'] if root.get('is_volume') else [root]):
            if chapter['title'] in outside_format:
                for _, target in _walk([chapter]):
                    candidates[target['target_id']] = (chapter['chapter_id'], outside_format[chapter['title']])
    positions = {}
    for path, target in _walk(outline['chapters'], skip_covers=True):
        opening = _compact(target.get('purpose', ''))[:_OPENING]
        candidate, grids = candidates.get(target['target_id'], (None, ()))
        positions[f't{len(positions) + 1}'] = {'target': target, 'title_path': list(path),
                                               'kind': target['kind'], 'opening': opening,
                                               'outside_format': candidate is not None, 'candidate_chapter': candidate,
                                               'required_form': any(_template_identity(part) in grids
                                                                    for part in target.get('template_refs', []))}
    return positions


def placement_payload(positions, items, ids):
    return {'operation': PLACEMENT_OPERATION,
            'positions': [{'id': key, 'title_path': value['title_path'], 'kind': value['kind'],
                           'opening': value['opening'], **({'origin': 'outside_format'} if value['outside_format'] else {})}
                          for key, value in positions.items()],
            'requirements': [{'id': key, **items[key]['semantic']} for key in ids]}


def _output_fits(budget, ids, positions):
    widest = max([*positions, *NOT_REQUIRED], key=len)
    bound = len(json.dumps({'placements': [[key, widest] for key in ids]}, separators=(',', ':')).encode())
    return bound < budget.output_reserve_for(AuthoringTask.ITEM_PLACEMENT, tokenized=budget.tokenizer is not None)


def _placement_input_tokens(budget, payload):
    from docreader.tender.authoring_pipeline import _request

    measured = budget.require_request(
        _request(budget, AuthoringTask.ITEM_PLACEMENT, payload), AuthoringTask.ITEM_PLACEMENT)
    return measured["input_tokens"], measured["effective_input_budget_tokens"]


def plan_batches(budget, positions, items, fits):
    """Greedy source-order batches that stay within half the input window and the output reserve.

    BUG-333: an item that does not fit even alone with the position catalogue is returned unsent.
    A single item that fits the full window is still sent; only growing a batch stops at half.
    """
    def payload_ok(ids):
        return (len(ids) <= PLACEMENT_BATCH_ITEMS and fits(placement_payload(positions, items, ids))
                and _output_fits(budget, ids, positions))

    def within_half(ids):
        if not payload_ok(ids):
            return False
        if len(ids) <= 1:
            return True
        used, limit = _placement_input_tokens(budget, placement_payload(positions, items, ids))
        return used <= int(limit * PLACEMENT_BATCH_INPUT_FRACTION)

    keys = list(items)
    batches, unsent, index = [], [], 0
    while index < len(keys):
        chunk = keys[index:index + PLACEMENT_BATCH_ITEMS]
        if within_half(chunk):
            batches.append(chunk)
            index += len(chunk)
            continue
        if not payload_ok(chunk[:1]):
            unsent.append(chunk[0])
            index += 1
            continue
        low, high = 1, len(chunk) - 1
        while low < high:
            mid = (low + high + 1) // 2
            if within_half(chunk[:mid]):
                low = mid
            else:
                high = mid - 1
        batches.append(chunk[:low])
        index += low
    return batches, unsent


def decode_placements(raw, ids, positions):
    def reject(detail):
        raise ModelContractError([{'path': ['placements'], 'code': 'item_placement_invalid', 'detail': detail}])

    try:
        parsed = ItemPlacementResponse.model_validate(raw).model_dump()
    except ValidationError as error:
        raise ModelContractError([{'path': list(e['loc']), 'code': e['type'], 'detail': e['msg']}
                                  for e in error.errors(include_input=False)]) from None
    seen = {}
    for identity, value in parsed['placements']:
        if identity not in ids or identity in seen:
            reject('每个本批事项 ID 恰好出现一次，不能有本批以外的 ID。')
        if isinstance(value, str) and value.strip().lower() == 'null':
            value = None
        if value is not None and value not in positions and value not in NOT_REQUIRED:
            reject('右值只能是本批位置 ID、规定的理由代码或 null。')
        seen[identity] = value
    if seen.keys() != set(ids):
        reject('本批每个事项都要回一行。')
    return seen


def bind_placements(outline, positions, items, decided):
    """Write accepted placements into the outline; pending items stay out for the ledger."""
    outline = deepcopy(outline)
    by_target = {}
    for path, target in _walk(outline['chapters']):
        by_target[target['target_id']] = target
    excluded = {}
    pending_evaluation = []
    for key, value in decided.items():
        item = items[key]
        ref = deepcopy(item['rule_ref'])
        if value in positions:
            by_target[positions[value]['target']['target_id']].setdefault('rule_refs', []).append(ref)
        elif value == 'document_wide':
            outline.setdefault('constraint_rule_refs', []).append(ref)
        elif value == 'evaluation':
            pending_evaluation.append(ref)
        elif value in NOT_REQUIRED:
            excluded.setdefault(value, []).append(ref)
    for code, refs in excluded.items():
        outline['issues'].append({'code': 'outline_rule_excluded', 'rule_refs': refs,
                                  'detail': f'事项归属：{NOT_REQUIRED[code]}'})
    if pending_evaluation:
        outline['issues'].append({'code': 'outline_rule_unassigned', 'rule_refs': pending_evaluation,
                                  'detail': EVALUATION_PENDING_DETAIL})
    outside = {value['candidate_chapter'] for value in positions.values() if value['outside_format']}
    required = {value['target']['target_id'] for value in positions.values() if value['required_form']}

    def keep(chapter):
        return chapter['chapter_id'] not in outside or any(
            target['target_id'] in required or target.get('rule_refs') for _, target in _walk([chapter]))

    outline['chapters'] = [chapter for chapter in outline['chapters'] if keep(chapter)]
    for chapter in outline['chapters']:
        if chapter.get('is_volume'):
            chapter['children'] = [child for child in chapter['children'] if keep(child)]
    return outline


async def place_items(outline, items, budget, execute, ask, fits, outside_format=None):
    """One logical request per batch. A failed batch leaves its items pending."""
    keyed = {f'r{number}': row for number, row in enumerate(items, start=1)}
    positions = placement_positions(outline, outside_format)
    if not keyed or not positions:
        return outline, {'batches': 0, 'failed': 0}
    batches, unsent = await asyncio.to_thread(plan_batches, budget, positions, keyed, fits)

    async def one(ids):
        payload = placement_payload(positions, keyed, ids)
        try:
            return await ask(execute, budget, AuthoringTask.ITEM_PLACEMENT, payload,
                             lambda raw: decode_placements(raw, ids, positions))
        except (ModelWorkUnitExhausted, ModelBudgetError):
            return None

    from docreader.tender.authoring_pipeline import _plan

    await _plan(execute, AuthoringTask.ITEM_PLACEMENT, len(batches), 'item-placement')
    results = await asyncio.gather(*(one(ids) for ids in batches))
    decided = {key: value for result in results if result for key, value in result.items()}
    bound = bind_placements(outline, positions, keyed, decided)
    failed = [ids for ids, result in zip(batches, results) if result is None]
    pending = {key for ids in failed for key in ids} | set(unsent)
    if pending:
        bound['issues'].append({'code': 'outline_item_placement_failed', 'rule_refs': [
            deepcopy(keyed[key]['rule_ref']) for key in keyed if key in pending],
            'detail': '事项归属批次未被采纳或连同位置目录超出单次请求预算，相关事项留待核。'})
    return bound, {'batches': len(batches), 'failed': len(failed)}
