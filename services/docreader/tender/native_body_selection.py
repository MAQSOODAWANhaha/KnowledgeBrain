"""Finite native-body ID choices and complete-request packing; no model or I/O."""
from copy import deepcopy
import json
import re
from typing import Any, Callable, cast

from docreader.llm.request import ModelBudgetError, ModelContractError

CONTRACT = 'native_body_contiguous_selection_v4'
# Invalid-JSON feedback is <=512 serialized characters (<=2048 UTF-8 bytes).
# R semantic feedback has one <=256-character host error and no copied output.
# Include JSON insertion and tokenizer-boundary allowance in the input bound.
CORRECTION_HEADROOM_TOKENS = 4 * 512 + 256


def _json(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':'), allow_nan=False)


def _reject(detail):
    raise ModelContractError([{'path': [], 'code': 'native_body_selection_invalid', 'detail': detail[:256]}],
                             feedback_fragments=[])


def _parts(text):
    """Reversible candidate boundaries, not a claim of semantic completeness."""
    if not text.strip():
        return []
    ends = []
    for match in re.finditer(r'[。；！？]', text):
        end = match.end()
        while end < len(text) and text[end].isspace():
            end += 1
        if end not in ends:
            ends.append(end)
    if text and (not ends or ends[-1] != len(text)):
        ends.append(len(text))
    return [(start, end) for start, end in zip([0, *ends], ends) if start < end]


def _positive(value):
    return type(value) is int and value > 0


class NativeBodySelectionView:
    def __init__(self, cells, units):
        self.cells, self.units = deepcopy(list(cells)), deepcopy(list(units))
        self.by_cell = {c['cell_id']: c for c in self.cells}
        self.parts = {c['cell_id']: _parts(c['text']) for c in self.cells}
        if len(self.by_cell) != len(self.cells) or any(not _positive(c['cell_id']) for c in self.cells):
            raise ValueError('native cells require unique positive integer IDs')
        self.order = {c['cell_id']: i for i, c in enumerate(self.cells)}
        ids = [u['unit_id'] for u in self.units]
        if len(ids) != len(set(ids)) or any(not _positive(i) for i in ids):
            raise ValueError('native units require unique positive integer IDs')
        for cell in self.cells:
            kind = cell.get('source_kind', 'native_grid')
            if kind == 'native_prose':
                if (any(k in cell for k in ('table_id', 'row', 'column', 'row_span', 'col_span')) or
                        not cell.get('document_id') or not cell.get('native_unit_id') or not cell.get('native_range_id') or
                        type(cell.get('page')) is not int or cell['page'] < 0):
                    raise ValueError('native prose requires its real range identity without invented grid geometry')
                continue
            if kind != 'native_grid':
                raise ValueError('unknown native source kind')
            if (type(cell['text']) is not str or type(cell['row']) is not int or cell['row'] < 0 or
                    type(cell['column']) is not int or cell['column'] < 0 or
                    not _positive(cell.get('row_span', 1)) or not _positive(cell.get('col_span', 1))):
                raise ValueError('invalid frozen native cell')
        for unit in self.units:
            islands = unit['islands']
            ids = [i['island_id'] for i in islands]
            if not ids or len(ids) != len(set(ids)) or any(not _positive(i) for i in ids):
                raise ValueError('native islands require unique positive integer IDs')
            kinds = {self.by_cell[c].get('source_kind', 'native_grid') for island in islands for c in island['cell_ids'] if c in self.by_cell}
            if len(kinds) > 1:
                raise ValueError('one body unit cannot mix grid and native prose source order')
            if kinds == {'native_prose'} and len({c for island in islands for c in island['cell_ids']}) > 1:
                raise ValueError('this body contract selects one native prose range per unit')
            for island in islands:
                cells = island['cell_ids']
                if (any(not _positive(i) or i not in self.by_cell for i in cells) or
                        len(cells) != len(set(cells)) or cells != sorted(cells, key=cast(Callable[[Any], Any], self.order.get))):
                    raise ValueError('native island cells must follow the supplied frozen source order')
            if self._is_prose(unit):
                anchors = unit.get('anchor_spans')
                allowed = {c for island in islands for c in island['cell_ids']}
                if not isinstance(anchors, list) or not anchors:
                    raise ValueError('native prose requires its host-known original anchor spans')
                for anchor in anchors:
                    if (type(anchor) is not list or len(anchor) != 3 or type(anchor[0]) is not int or anchor[0] not in allowed
                            or any(type(i) is not int for i in anchor[1:])
                            or not 0 <= anchor[1] < anchor[2] <= len(self.by_cell[anchor[0]]['text'])):
                        raise ValueError('native prose anchor is outside its immutable source range')

    def _is_prose(self, unit):
        return any(self.by_cell[c].get('source_kind') == 'native_prose' for i in unit['islands'] for c in i['cell_ids'])

    def payload(self):
        cells = [{**c, 'part_boundaries': [[i, start, end] for i, (start, end) in enumerate(
            self.parts[c['cell_id']], 1)]} for c in self.cells]
        return deepcopy({'contract': CONTRACT, 'cells': cells, 'units': self.units,
            'body_fields': ['cell_id', 'first_part', 'last_part', 'join_id'],
            'join_ids': {'0': 'paragraphs', '1': 'continuation'}})

    def subset(self, unit_ids):
        selected = set(unit_ids)
        if len(selected) != len(unit_ids) or not selected <= {u['unit_id'] for u in self.units}:
            raise ValueError('unknown or repeated native unit')
        units = [u for u in self.units if u['unit_id'] in selected]
        cells = {c for u in units for i in u['islands'] for c in i['cell_ids']}
        # Context includes all cells of touched tables, including merged neighbours.
        tables = {(self.by_cell[c].get('document_id'), self.by_cell[c]['table_id']) for c in cells
                  if self.by_cell[c].get('source_kind') != 'native_prose'}
        return type(self)([c for c in self.cells if (c['cell_id'] in cells if c.get('source_kind') == 'native_prose'
            else (c.get('document_id'), c['table_id']) in tables)], units)

    def response_schema(self):
        def integer(maximum):
            return {'type': 'integer', 'minimum': 1, 'maximum': max(maximum, 1)}
        def obj(properties):
            return {'type': 'object', 'properties': properties,
                    'required': list(properties), 'additionalProperties': False}
        def array(items, maximum):
            return {'type': 'array', 'items': items, 'maxItems': maximum}
        islands = [i for u in self.units for i in u['islands']]
        interval = {'type': 'array', 'prefixItems': [integer(max(self.by_cell, default=1)),
            integer(max(map(len, self.parts.values()), default=1)),
            integer(max(map(len, self.parts.values()), default=1)),
            {'type': 'integer', 'enum': [0, 1]}], 'minItems': 4, 'maxItems': 4}
        identity = integer(max((i['island_id'] for i in islands), default=1))
        grid = obj({'island_id': identity, 'state': {'enum': ['selected', 'unresolved']},
                    'body': array(interval, max((len(i['cell_ids']) for i in islands), default=0))})
        context_interval = {**interval, 'prefixItems': interval['prefixItems'][:3], 'minItems': 3, 'maxItems': 3}
        prose = obj({'island_id': identity, 'state': {'enum': ['selected', 'unresolved']},
                     'body': array(interval, 1), 'context': array(context_interval, 2)})
        kinds = {self._is_prose(u) for u in self.units}
        island = prose if kinds == {True} else grid if kinds != {False, True} else {'anyOf': [grid, prose]}
        unit = obj({'unit_id': integer(max((u['unit_id'] for u in self.units), default=1)),
                    'islands': array(island, max((len(u['islands']) for u in self.units), default=0))})
        choices = array(unit, len(self.units))
        choices['minItems'] = len(self.units)
        return obj({'native_choices': choices})

    def output_bound(self):
        """Bound canonical JSON over a superset of host-accepted choices.

        Selecting every eligible cell at its largest part ID maximizes bytes per
        island. Shared cells may make the combined witness unattainable across
        islands; excluding those overlaps only decreases this conservative bound.
        Provider whitespace, reasoning and elapsed time are outside this contract.
        """
        choices = []
        for unit in self.units:
            islands = []
            for island in unit['islands']:
                body = [[c, len(self.parts[c]), len(self.parts[c]), 1] for c in island['cell_ids'] if self.parts[c]]
                selected = {'island_id': island['island_id'], 'state': 'selected' if body else 'unresolved', 'body': body}
                if self._is_prose(unit):
                    selected['context'] = [[c, len(self.parts[c]), len(self.parts[c])]
                        for c in island['cell_ids'] if self.parts[c] for _ in range(2)]
                islands.append(selected)
            choices.append({'unit_id': unit['unit_id'], 'islands': islands})
        return {'method': 'canonical_json_utf8_bytes_conservative_superset_v1',
                'token_upper_bound': len(_json({'native_choices': choices}).encode()),
                'provider_output_cap': False}

    def accept(self, raw):
        if not isinstance(raw, dict) or set(raw) != {'native_choices'} or type(raw['native_choices']) is not list:
            _reject('Return only the complete native_choices array.')
        choices = self._align_choices(raw['native_choices'])
        units, pending = [], []
        for expected, choice in zip(self.units, choices):
            if (not isinstance(choice, dict) or set(choice) != {'unit_id', 'islands'} or
                    type(choice['unit_id']) is not int or choice['unit_id'] != expected['unit_id'] or
                    type(choice['islands']) is not list or len(choice['islands']) != len(expected['islands'])):
                _reject('Return the exact supplied unit and island membership.')
            materialized, selected, previous, previous_cell = [], {}, None, None
            prose = self._is_prose(expected)
            contexts, context_pieces, previous_context = {}, [], None
            for given, island in zip(expected['islands'], choice['islands']):
                if (not isinstance(island, dict) or set(island) != ({'island_id', 'state', 'body', 'context'} if prose else {'island_id', 'state', 'body'}) or
                        type(island['island_id']) is not int or island['island_id'] != given['island_id'] or
                        island['state'] not in ('selected', 'unresolved') or type(island['body']) is not list):
                    _reject('Return every supplied island once; source text must not be echoed.')
                body, seen = [], []
                for interval in island['body']:
                    if (type(interval) is not list or len(interval) != 4 or any(not _positive(i) for i in interval[:3]) or
                            type(interval[3]) is not int or interval[3] not in (0, 1)):
                        _reject('Choose integer cell, inclusive part IDs, and an explicit 0/1 join ID.')
                    cell_id, first, last, join = interval
                    if (cell_id not in given['cell_ids'] or cell_id in seen or
                            not 1 <= first <= last <= len(self.parts[cell_id])):
                        _reject('Choose at most one valid forward interval per eligible cell.')
                    seen.append(cell_id)
                    start, end = self.parts[cell_id][first - 1][0], self.parts[cell_id][last - 1][1]
                    position = (self.order[cell_id], start)
                    if previous is not None and position < previous:
                        _reject('Source continuation must be forward without overlap across islands.')
                    document = self.by_cell[cell_id].get('document_id')
                    if join and (previous_cell is None or not document or
                            document != self.by_cell[previous_cell].get('document_id')):
                        _reject('Continuation requires a preceding body part in the same explicitly identified document.')
                    previous, previous_cell = (self.order[cell_id], end), cell_id
                    selected.setdefault(cell_id, []).append((start, end))
                    body.append({**self._slice(cell_id, start, end),
                                 'source_join': 'continuation' if join else 'paragraphs'})
                if seen != sorted(seen, key=cast(Callable[[Any], Any], self.order.get)):
                    _reject('Choose cells in their supplied frozen source order.')
                if (island['state'] == 'selected') != bool(body):
                    _reject('Selected requires body; unresolved requires an empty body.')
                if prose:
                    if type(island['context']) is not list or len(island['context']) > 2:
                        _reject('Choose at most two explicit context intervals per native prose island.')
                    if island['state'] == 'unresolved' and island['context']:
                        _reject('Unresolved prose must leave both body and context empty.')
                    for interval in island['context']:
                        if type(interval) is not list or len(interval) != 3 or any(not _positive(i) for i in interval):
                            _reject('Choose supplied integer cell and inclusive part IDs for context.')
                        cell_id, first, last = interval
                        if cell_id not in given['cell_ids'] or not 1 <= first <= last <= len(self.parts[cell_id]):
                            _reject('Context must stay inside its supplied native range.')
                        start, end = self.parts[cell_id][first - 1][0], self.parts[cell_id][last - 1][1]
                        if previous_context is not None and (self.order[cell_id], start) < previous_context:
                            _reject('Explicit context must run forward without repeats or overlap.')
                        previous_context = (self.order[cell_id], end)
                        contexts.setdefault(cell_id, []).append((start, end))
                        context_pieces.append(self._slice(cell_id, start, end))
                if island['state'] == 'unresolved':
                    pending.append({'unit_id': expected['unit_id'], 'island_id': given['island_id'],
                        'reason': 'native_body_unresolved', 'outlet': 'manual_source_review', 'enterprise_confirmed': False})
                materialized.append({'island_id': given['island_id'], 'state': island['state'], 'body': body})
            if prose:
                for cell_id, intervals in contexts.items():
                    if any(low < end and start < high for low, high in intervals for start, end in selected.get(cell_id, [])):
                        _reject('Source body and explicit context must be disjoint.')
                if all(i['state'] == 'selected' for i in materialized) and not self._anchor_covered(expected, selected, contexts):
                    for island in materialized:
                        island['state'] = 'unresolved'
                        island['body'] = []
                        pending.append({'unit_id': expected['unit_id'], 'island_id': island['island_id'],
                            'reason': 'native_body_anchor_uncovered', 'outlet': 'manual_source_review',
                            'enterprise_confirmed': False})
                    context_pieces = []
            units.append({'unit_id': expected['unit_id'], 'islands': materialized,
                          'context': context_pieces if prose else self._context(selected)})
        return {'units': units, 'pending': pending, 'complete': not pending}

    def _align_choices(self, choices):
        if type(choices) is not list:
            _reject('Return every supplied unit exactly once, in source order.')
        by_unit = {}
        for choice in choices:
            if not isinstance(choice, dict) or type(choice.get('unit_id')) is not int or choice['unit_id'] in by_unit:
                _reject('Return every supplied unit exactly once, in source order.')
            by_unit[choice['unit_id']] = choice
        ordered = []
        for expected in self.units:
            prose = self._is_prose(expected)
            choice = by_unit.pop(expected['unit_id'], None)
            if choice is None:
                choice = {'unit_id': expected['unit_id'], 'islands': [
                    self._unresolved_island(island, prose) for island in expected['islands']]}
            else:
                choice = {**choice, 'islands': self._align_islands(expected, choice.get('islands'), prose)}
            ordered.append(choice)
        if by_unit:
            _reject('Return every supplied unit exactly once, in source order.')
        return ordered

    def _align_islands(self, unit, islands, prose):
        if type(islands) is not list:
            _reject('Return every supplied island once; source text must not be echoed.')
        by_island = {}
        for island in islands:
            if not isinstance(island, dict) or type(island.get('island_id')) is not int or island['island_id'] in by_island:
                _reject('Return every supplied island once; source text must not be echoed.')
            by_island[island['island_id']] = island
        ordered = []
        for expected in unit['islands']:
            ordered.append(by_island.pop(expected['island_id'], self._unresolved_island(expected, prose)))
        if by_island:
            _reject('Return every supplied island once; source text must not be echoed.')
        return ordered

    @staticmethod
    def _unresolved_island(island, prose):
        unresolved = {'island_id': island['island_id'], 'state': 'unresolved', 'body': []}
        if prose:
            unresolved['context'] = []
        return unresolved

    def _anchor_covered(self, unit, body, context):
        for cell, start, end in unit['anchor_spans']:
            text, cursor = self.by_cell[cell]['text'], start
            for low, high in sorted([*body.get(cell, []), *context.get(cell, [])]):
                low, high = max(start, low), min(end, high)
                if low >= high:
                    continue
                if text[cursor:low].strip():
                    return False
                cursor = max(cursor, high)
            if text[cursor:end].strip():
                return False
        return True

    def _slice(self, cell, start, end):
        return {'cell_id': cell, 'start': start, 'end': end, 'text': self.by_cell[cell]['text'][start:end]}

    def _context(self, selected):
        touched = [self.by_cell[c] for c in selected]
        result = []
        for cell in self.cells:
            if cell.get('source_kind') == 'native_prose':
                continue
            relevant = any(other.get('source_kind') != 'native_prose' and cell.get('document_id') == other.get('document_id') and cell['table_id'] == other['table_id'] and
                    cell['row'] < other['row'] + other.get('row_span', 1) and
                    other['row'] < cell['row'] + cell.get('row_span', 1) for other in touched)
            if not relevant:
                continue
            start = 0
            for low, high in [*selected.get(cell['cell_id'], []), (len(cell['text']), len(cell['text']))]:
                if start < low:
                    result.append(self._slice(cell['cell_id'], start, low))
                start = high
        return result


def plan_native_body_packs(view, budget, request_factory, *, task):
    """Greedy whole-unit packing; unfit units retain a named manual exit."""
    packs, pending, current = [], [], []
    def admitted(ids):
        candidate = view.subset(ids)
        request = request_factory(candidate)
        measured = budget.require_request(request, task)
        bound = candidate.output_bound()
        correction_upper = measured['input_tokens'] + CORRECTION_HEADROOM_TOKENS
        if (measured['fits'] is not True or bound['token_upper_bound'] > measured['output_reserve']
                or correction_upper > measured['effective_input_budget_tokens']):
            raise ModelBudgetError('Complete native body selection needs proven input and output capacity.')
        return {'view': candidate, 'request': request, 'budget': measured, 'output_bound': bound,
                'correction_input_upper_bound_tokens': correction_upper}
    accepted = None
    for unit in view.units:
        identity = unit['unit_id']
        try:
            candidate = admitted([*current, identity])
        except ModelBudgetError:
            if current:
                packs.append(accepted)
                current, accepted = [], None
            try:
                candidate = admitted([identity])
            except ModelBudgetError:
                pending.append({'unit_id': identity, 'reason': 'complete_native_body_unit_over_budget',
                    'outlet': 'manual_source_review', 'enterprise_confirmed': False})
                continue
        current.append(identity)
        accepted = candidate
    if current:
        packs.append(accepted)
    return {'packs': packs, 'pending_units': pending, 'complete': not pending, 'model_calls': 0}
