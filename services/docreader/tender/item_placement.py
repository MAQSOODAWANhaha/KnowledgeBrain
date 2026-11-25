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
# A whole 715-item ledger fit the window but the model answered three rows; the pilot's 345 passed.
PLACEMENT_BATCH_ITEMS = 240


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


def submission_form_nodes(documents, plan, items, format_pages):
    """Plan nodes for named submission forms that the bid-format chapter does not contain."""
    named = {_compact(node.get('name')) for node in plan.get('nodes', [])}
    wanted = {}
    for row in items:
        if row['semantic'].get('kind') != 'submission_requirement':
            continue
        for name in (row['semantic'].get('submission') or {}).get('documents') or []:
            outside = [e for e in row['evidence'] if e['page'] is not None
                       and (e['document_id'], e['page']) not in format_pages]
            if _compact(name) and _compact(name) not in named and outside:
                wanted.setdefault(_compact(name), {'name': name, 'rows': [], 'evidence': []})
                wanted[_compact(name)]['rows'].append(row)
                wanted[_compact(name)]['evidence'].extend(outside)
    by_id = {document['document_id']: document for document in documents}
    nodes = []
    for key, entry in wanted.items():
        heading = None
        for document_id, page in sorted({(e['document_id'], e['page']) for e in entry['evidence']}):
            document = by_id.get(document_id)
            if document is None:
                continue
            for candidate in (page - 1, page):
                if 0 <= candidate < len(document['pages']):
                    lines = document['pages'][candidate].splitlines()
                    found = next((i for i, line in enumerate(lines) if _compact(line) == key), None)
                    if found is not None:
                        heading = (document_id, candidate, found)
                        break
            if heading:
                break
        if heading is None:
            continue
        document_id, page, line = heading
        pages = by_id[document_id]['pages']
        last = max(e['page'] for e in entry['evidence'] if e['document_id'] == document_id)
        windows = []
        for current in range(page, last + 1):
            count = len(pages[current].splitlines())
            start = line if current == page else 0
            if count > start:
                windows.append({'document_id': document_id, 'page': current, 'start_line': start, 'end_line': count - 1})
        nodes.append({'node': {'id': f'submission-{len(nodes) + 1}', 'parent_id': None, 'response_kind': 'form',
                               'name': entry['name'],
                               'heading': {'document_id': document_id, 'page': page, 'line': line},
                               'body_windows': windows},
                      'point': heading})
    return nodes


def add_submission_forms(plan, documents, items, format_pages):
    """Insert candidate forms in source order. Returns their names; placement decides whether each stays."""
    extra = submission_form_nodes(documents, plan, items, format_pages)
    if not extra:
        return plan, []
    plan = deepcopy(plan)
    entries = plan['composition']['entries']
    items_in_scope = plan['scopes'][0]['items']
    for added in extra:
        position = sum(1 for entry in entries if (entry['name']['document_id'], entry['name']['page'],
                                                  entry['name']['line']) < added['point'])
        for item in items_in_scope:
            if item['entry'] >= position:
                item['entry'] += 1
        entries.insert(position, {'name': deepcopy(added['node']['heading'])})
        plan['nodes'].append(added['node'])
        items_in_scope.append({'node_id': added['node']['id'], 'entry': position, 'depth': 'self',
                               'presentation': 'section', 'clause': []})
    return plan, [added['node']['name'] for added in extra]


def placement_positions(outline, outside_format=()):
    positions = {}
    for path, target in _walk(outline['chapters'], skip_covers=True):
        opening = _compact(target.get('purpose', ''))[:_OPENING]
        positions[f't{len(positions) + 1}'] = {'target': target, 'title_path': list(path),
                                               'kind': target['kind'], 'opening': opening,
                                               'outside_format': path[0] in outside_format}
    return positions


def placement_payload(positions, items, ids):
    return {'operation': PLACEMENT_OPERATION,
            'positions': [{'id': key, 'title_path': value['title_path'], 'kind': value['kind'],
                           'opening': value['opening'], **({'origin': 'outside_format'} if value['outside_format'] else {})}
                          for key, value in positions.items()],
            'requirements': [{'id': key, **items[key]['semantic'],
                              'evidence': [{'page': None if e['page'] is None else e['page'] + 1, 'quote': e['quote']}
                                           for e in items[key]['evidence']]} for key in ids]}


def _output_fits(budget, ids, positions):
    widest = max([*positions, *NOT_REQUIRED], key=len)
    bound = len(json.dumps({'placements': [[key, widest] for key in ids]}, separators=(',', ':')).encode())
    return bound < budget.output_reserve_for(AuthoringTask.ITEM_PLACEMENT, tokenized=budget.tokenizer is not None)


def plan_batches(budget, positions, items, fits):
    """Greedy source-order batches that fit the input window and the output reserve.

    BUG-333: an item that does not fit even alone with the position catalogue is returned unsent.
    """
    def admitted(ids):
        return (len(ids) <= PLACEMENT_BATCH_ITEMS and fits(placement_payload(positions, items, ids))
                and _output_fits(budget, ids, positions))

    batches, current, unsent = [], [], []
    for key in items:
        if admitted([*current, key]):
            current.append(key)
            continue
        if current:
            batches.append(current)
        current = [key] if admitted([key]) else []
        if not current:
            unsent.append(key)
    if current:
        batches.append(current)
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
    # A candidate form outside the bid-format chapter stays only if an item is placed in it.
    outline['chapters'] = [chapter for chapter in outline['chapters'] if not any(
        value['outside_format'] and value['title_path'][0] == chapter['title'] for value in positions.values())
        or any(target.get('rule_refs') for _, target in _walk([chapter]))]
    return outline


async def place_items(outline, items, budget, execute, ask, fits, outside_format=()):
    """One logical request per batch. A failed batch leaves its items pending."""
    keyed = {f'r{number}': row for number, row in enumerate(items, start=1)}
    positions = placement_positions(outline, set(outside_format))
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
