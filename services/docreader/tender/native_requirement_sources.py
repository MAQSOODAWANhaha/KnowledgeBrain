"""Exact native requirement sources; no directory placement or pipeline side effects.

The two small text/fragment routines follow ResponseFormView's stable contract.
This module owns neither response-form layout nor the directory disposition engine.
"""
from copy import deepcopy
import hashlib
import json
import re
from typing import Annotated, Literal

from pydantic import AfterValidator, Field, StrictInt, ValidationError, create_model

from docreader.llm.request import ModelBudgetError, ModelContractError, build_request
from docreader.tender.authoring_excerpts import source_excerpts_text
from docreader.tender.directory_compile import (
    ClassificationPlan, DirectoryWindow, RequirementAdjudication, RequirementAssignment, RequirementPlan,
)
from docreader.tender.evidence.references import EvidenceRepository, fragment, selector
from docreader.tender.schema import Contract

CONTRACT = 'native_requirement_sources_v1'
REQUIREMENT_OPERATION = 'select_native_requirement_sources_v1'
CLASSIFICATION_OPERATION = 'classify_with_native_requirement_cases_v1'
_JOIN = Literal['paragraphs', 'continuation']
_ROLE = Literal['requirement', 'context', 'external_action', 'evaluator_procedure',
                'reference', 'navigation', 'layout', 'already_carried', 'unresolved']
_SCOPE = Literal['chapter', 'chapters', 'document', 'unassigned']
_REFS = Annotated[list[StrictInt], Field(min_length=1)]
_LITERAL_BODY_DOMAIN = 'grid_and_certified_literal_body_v1'


def _json(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':'), allow_nan=False)


def _hash(value):
    return hashlib.sha256(_json(value).encode()).hexdigest()


def _reject(detail, *, path=None):
    raise ModelContractError([{'path': path or [], 'code': 'native_requirement_source_invalid', 'detail': detail}],
                             feedback_fragments=[] if path else None)


def _parse(model, raw):
    try:
        return model.model_validate(raw).model_dump()
    except ValidationError as error:
        raise ModelContractError([{'path': list(e['loc']), 'code': e['type'], 'detail': e['msg']}
                                  for e in error.errors(include_input=False)]) from None


class NativeContext(Contract):
    refs: _REFS
    parent_id: StrictInt | None = None
    source_join: _JOIN = 'paragraphs'


class NativeRequirementAssignment(Contract):
    body_refs: _REFS
    context_id: StrictInt | None = None
    source_join: _JOIN = 'paragraphs'
    scope: _SCOPE
    chapter_ids: list[str] = Field(default_factory=list)


class NativeDisposition(Contract):
    refs: _REFS
    kind: _ROLE


class NativeRequirementPlan(RequirementPlan):
    contexts: list[NativeContext] = Field(default_factory=list)
    native_assignments: list[NativeRequirementAssignment] = Field(default_factory=list)
    native_dispositions: list[NativeDisposition] = Field(default_factory=list)


class NativeRequirementAdjudication(RequirementAdjudication):
    source_join: _JOIN | None = None


class NativeClassificationPlan(ClassificationPlan):
    requirement_adjudications: list[NativeRequirementAdjudication] = Field(default_factory=list)


def _lines(text):
    result, offset, prefix = [], 0, ''
    for line in text.splitlines(keepends=True):
        if line.strip():
            result.append((offset - len(prefix), prefix + line))
            prefix = ''
        elif result:
            result[-1] = (result[-1][0], result[-1][1] + line)
        else:
            prefix += line
        offset += len(line)
    return result


def _page(unit):
    for loc in (unit.get('physical_locator'), unit.get('locator')):
        if isinstance(loc, dict) and type(loc.get('page_ordinal')) is int:
            return loc['page_ordinal']
    match = re.search(r'(?:^|:)page:(\d+)(?:$|:)', unit['unit_id'])
    return int(match[1]) if match else None


def _scope(scope, ids):
    if len(ids) != len(set(ids)):
        _reject('Choose each chapter once.')
    if ((scope == 'chapter' and len(ids) != 1) or (scope == 'chapters' and not ids)
            or (scope not in {'chapter', 'chapters'} and ids)):
        _reject('Chapter choices must match the explicit disposition scope.')


def _legacy_points(block, document_map):
    reverse = {value: key for key, value in document_map.items()}
    points = set()
    for page in block:
        document = page['document_id']
        alias = document if document in document_map else reverse.get(document)
        if alias is None:
            _reject('Legacy prose block belongs to an unknown registered document.')
        for line in page['lines']:
            number = line['line'] if isinstance(line, dict) else line[0]
            points.add((alias, page['page'], number))
    return points


def _prose_points(block, document_map, grid_units):
    """Only host-certified non-grid layout lines belong to the legacy path."""
    reverse = {value: key for key, value in document_map.items()}
    points = set()
    for page in block:
        document = page['document_id']
        alias = document if document in document_map else reverse.get(document)
        for line in page['lines']:
            if not isinstance(line, dict) or line.get('source_role') != 'prose':
                continue
            if (document_map[alias], line.get('unit_id')) in grid_units:
                _reject('A known grid layout line cannot be certified as legacy prose.')
            points.add((alias, page['page'], line['line']))
    return points


class SourceLedger:
    def __init__(self, artifacts, documents, *, chapters=(), pages=None, repository=None):
        self.artifacts = artifacts
        self.repository = repository or EvidenceRepository(artifacts)
        self.documents = deepcopy(documents)
        self.chapter_map = {f'c{i}': c['id'] for i, c in enumerate(chapters, 1)}
        if len(set(self.chapter_map.values())) != len(self.chapter_map):
            _reject('Chapter identities must be unique.')
        self.chapter_payload = [{**{k: deepcopy(v) for k, v in c.items() if k not in {'id', 'parent_id'}},
                                 'id': alias} for alias, c in zip(self.chapter_map, chapters)]
        reverse = {v: k for k, v in self.chapter_map.items()}
        for item, original in zip(self.chapter_payload, chapters):
            if original.get('parent_id') is not None:
                if original['parent_id'] not in reverse:
                    _reject('Every chapter parent must exist in the supplied chapter inventory.')
                item['parent_id'] = reverse[original['parent_id']]
        self.document_map = {f'd{i}': d['document_id'] for i, d in enumerate(self.documents, 1)}
        if len(set(self.document_map.values())) != len(self.document_map):
            _reject('Registered document identities must be unique.')
        if pages is not None and not set(pages) <= set(self.document_map.values()):
            _reject('Selected pages must belong to registered frozen documents.')
        self.atoms, self.ranges, self.grid_units = {}, [], set()
        self.literal_locations, self.literal_body_refs, self._literal_atoms, self._refs_by_range = {}, {}, {}, {}
        self.literal_predecessors = {}
        self.reviewed_body_refs, self.reviewed_partial_refs = set(), set()
        for document_number, document in enumerate(self.documents, 1):
            if pages is not None and document['document_id'] not in pages:
                continue
            index_ref = document['source_index_ref']
            index = self.repository.source_index(index_ref)
            if any(index.document[k] != document[k] for k in ('document_id', 'document_version', 'original_ref')):
                _reject('Frozen source index belongs to another document/version/original.')
            units = {u['unit_id']: u for u in index.raw['units']}
            self.grid_units.update((document['document_id'], base['unit_id']) for base in index.ranges
                                   if base.get('text_basis') == 'grid_row')
            found_pages = set()
            for source_ordinal, base in enumerate(index.ranges):
                unit = units[base['unit_id']]
                page = _page(unit)
                if page is None and (unit.get('locator') or {}).get('locator_kind') == 'attachment':
                    continue  # Registered attachment metadata, not a physical-page source range.
                if page is None:
                    _reject('Unlocated native source range needs named manual source review.')
                found_pages.add(page)
                if pages is not None and page not in pages[document['document_id']]:
                    continue
                ordinal = len(self.ranges)
                reference = {'source_index_ref': deepcopy(index_ref), 'unit_id': unit['unit_id']}
                common = {'document': document_number, 'page': page, 'unit': unit['ordinal']}
                def atoms(text, start, column=None):
                    result = []
                    for offset, line in _lines(text):
                        ref = len(self.atoms) + 1
                        self.atoms[ref] = {'base': deepcopy(base), 'source_ref': deepcopy(reference),
                            'start': start + offset, 'end': start + offset + len(line), 'range_ordinal': ordinal,
                            'document': document_number, 'page': page, 'row': base.get('row_start'), 'column': column}
                        key = index_ref['sha256'], unit['unit_id'], base['source_range_id']
                        self._refs_by_range.setdefault(key, []).append(ref)
                        if (base.get('text_basis') == 'pdf_literal' and start + offset == base['start']
                                and len(line) == base['end'] - base['start']):
                            self._literal_atoms[document['document_id'], unit['unit_id']] = ref
                        result.append([ref, line])
                    return result
                if base.get('text_basis') == 'grid_row':
                    cells = [{**{k: c[k] for k in ('column', 'row_span', 'col_span')},
                              'lines': atoms(c['text'], c['start'], c['column'])} for c in base['cell_intervals']]
                    wire = {**common, 'kind': 'native_row', 'row': base['row_start'],
                            'column_count': base['column_count'], 'cells': cells}
                else:
                    wire = {**common, 'kind': base['kind'], 'lines': atoms(base['text'], base['start'])}
                self.ranges.append({'ordinal': ordinal, 'source_ordinal': source_ordinal, 'source_range_count': len(index.ranges),
                                    'reference': reference, 'base': deepcopy(base), 'wire': wire})
            if pages is not None and not set(pages[document['document_id']]) <= found_pages:
                _reject('A requested physical page has no located native source; retain its layout for review.')
        if not self.ranges:
            _reject('No complete native source ranges were selected.')
        self.binding = self._source_binding = _hash({'contract': CONTRACT, 'documents': self.documents, 'pages': pages,
            'atoms': [(r, a['source_ref'], a['base']['source_range_id'], a['start'], a['end']) for r, a in self.atoms.items()],
            'ranges': [(r['reference'], r['base']['range_id']) for r in self.ranges]})

    def bind_literal_locations(self, locations, *, continuation_edges=()):
        """Bind whole lines and independently proven adjacent layout points."""
        seen = set()
        for key, (page, line) in locations.items():
            ref = self._literal_atoms.get(key)
            point = (key[0], page, line)
            if (ref is None or type(page) is not int or page != self.atoms[ref]['page']
                    or type(line) is not int or line < 0 or point in seen):
                _reject('Literal body locations must identify unique whole frozen lines on their physical page.')
            seen.add(point)
        predecessors, successors = {}, set()
        for before, after in continuation_edges:
            if before not in locations or after not in locations or before[0] != after[0]:
                _reject('Literal continuity requires two certified positions in one document.')
            first, second = self._literal_atoms[before], self._literal_atoms[after]
            start, end = (self.atoms[r]['range_ordinal'] for r in (first, second))
            page, line = locations[before]
            next_page, next_line = locations[after]
            ordered = next_page == page and next_line > line or next_page == page + 1
            if (not ordered or end != start + 1 or second in predecessors or first in successors
                    or not _continuous_ranges(self, [start, end])):
                _reject('Literal continuity cannot skip an intervening frozen nonblank source or branch.')
            predecessors[second] = first
            successors.add(first)
        self.literal_locations = dict(locations)
        self.literal_body_refs = {self._literal_atoms[key]: point for key, point in locations.items()}
        self.literal_predecessors = predecessors
        self.binding = (_hash({'source_binding': self._source_binding, 'literal_locations': sorted(locations.items()),
                               'literal_predecessors': sorted(predecessors.items())})
                        if locations else self._source_binding)

    def bind_reviewed_body_refs(self, excerpts):
        for excerpt in excerpts:
            reference = excerpt['source_ref']
            for selected in excerpt['selectors']:
                key = reference['source_index_ref']['sha256'], reference['unit_id'], selected['source_range_id']
                for ref in self._refs_by_range.get(key, ()):
                    atom = self.atoms[ref]
                    if selected['start'] <= atom['start'] and atom['end'] <= selected['end']:
                        self.reviewed_body_refs.add(ref)
                    elif selected['start'] < atom['end'] and atom['start'] < selected['end']:
                        self.reviewed_partial_refs.add(ref)
        self.reviewed_partial_refs.difference_update(self.reviewed_body_refs)
        if self.reviewed_body_refs or self.reviewed_partial_refs:
            self.binding = _hash({'native': self.binding, 'reviewed': sorted(self.reviewed_body_refs),
                                  'partial': sorted(self.reviewed_partial_refs)})

    def view(self, range_ordinals=None):
        return NativeSourceView(self, list(range(len(self.ranges))) if range_ordinals is None else range_ordinals)

    def excerpts(self, refs):
        if not refs or refs != sorted(set(refs)) or any(type(r) is not int or r not in self.atoms for r in refs):
            _reject('Choose visible source fragments once in forward source/cell order.')
        result, previous = [], None
        for ref in refs:
            a = self.atoms[ref]
            key = a['source_ref'], a['base']['source_range_id']
            if result and previous == key and result[-1]['selectors'][-1]['end'] == a['start']:
                start = result[-1]['selectors'][-1]['start']
                result[-1]['selectors'][-1] = selector(fragment(a['base'], start, a['end']))
            else:
                result.append({'source_ref': deepcopy(a['source_ref']),
                               'selectors': [selector(fragment(a['base'], a['start'], a['end']))]})
            previous = key
        return result

    def require_body(self, refs):
        if all(self.atoms[r]['base'].get('text_basis') == 'grid_row' for r in refs):
            return
        if any(r not in self.literal_body_refs for r in refs):
            _reject('One native body uses grid atoms or whole certified literal lines.')
        if any(self.literal_predecessors.get(second) != first for first, second in zip(refs, refs[1:])):
            _reject('Certified literal bodies must select contiguous lines in physical source order.')

    def _refs(self, excerpts):
        refs = []
        for excerpt in excerpts:
            for selected in excerpt['selectors']:
                reference = excerpt['source_ref']
                key = reference['source_index_ref']['sha256'], reference['unit_id'], selected['source_range_id']
                refs.extend(r for r in self._refs_by_range.get(key, ())
                            if selected['start'] <= self.atoms[r]['start'] < self.atoms[r]['end'] <= selected['end'])
        if not refs or self.excerpts(refs) != excerpts:
            _reject('Canonical source must resolve exactly to this ledger whole source atoms.')
        return refs

    def source(self, body_refs, contexts, *, source_join='paragraphs'):
        body = self.excerpts(body_refs)
        context_refs = [r for group in contexts for r in group['refs']]
        if len(context_refs) != len(set(context_refs)) or set(body_refs).intersection(context_refs):
            _reject('Canonical body and shared contexts must own disjoint exact source atoms.')
        all_refs = list(body_refs)
        context_parts = []
        for group in contexts:
            excerpts = self.excerpts(group['refs'])
            all_refs.extend(group['refs'])
            context_parts.append({'source_excerpts': excerpts, 'source_join': group.get('source_join', 'paragraphs'),
                'text': source_excerpts_text(self.artifacts, excerpts, source_join=group.get('source_join', 'paragraphs'), repository=self.repository)})
        if len({self.atoms[r]['source_ref']['source_index_ref']['sha256'] for r in all_refs}) != 1:
            _reject('One native source/case must stay in one frozen registered document.')
        value = {'kind': CONTRACT, 'source_binding': self.binding,
                 'body': {'source_excerpts': body, 'source_join': source_join,
                          'text': source_excerpts_text(self.artifacts, body, source_join=source_join, repository=self.repository)},
                 'contexts': context_parts}
        value['source_id'] = 'native-source-' + _hash(value)
        return value

    def validate_source(self, source):
        if source.get('kind') != CONTRACT or source.get('source_binding') != self.binding:
            _reject('Source authority/ledger binding changed; do not reinterpret a legacy source.')
        body = self._refs(source['body']['source_excerpts'])
        contexts = [{'refs': self._refs(g['source_excerpts']), 'source_join': g['source_join']} for g in source['contexts']]
        expected = self.source(body, contexts, source_join=source['body']['source_join'])
        if source != expected:
            _reject('Canonical source body/context/identity changed.')
        return deepcopy(expected)

    def case(self, source, *, origin, candidate_chapter_ids=()):
        self.validate_source(source)
        if len(set(candidate_chapter_ids)) != len(candidate_chapter_ids) or not set(candidate_chapter_ids) <= set(self.chapter_map.values()):
            _reject('Candidate chapter provenance must come from this source ledger inventory.')
        case = {'source': deepcopy(source), 'origin': origin,
                'candidate_chapter_ids': list(candidate_chapter_ids), 'enterprise_confirmed': False}
        case['id'] = 'requirement-native-' + _hash(case)
        return case

    def validate_case(self, case):
        if not isinstance(case, dict) or set(case) != {'source', 'origin', 'candidate_chapter_ids', 'enterprise_confirmed', 'id'}:
            _reject('Legacy/unknown case authority cannot be reinterpreted as a native case.')
        expected = self.case(case['source'], origin=case['origin'], candidate_chapter_ids=case['candidate_chapter_ids'])
        if case != expected:
            _reject('Pending native case identity/source changed.')
        return deepcopy(expected)


def native_requirement_model(payload):
    """Pure wire-payload Schema dispatch; performs no source/artifact I/O."""
    refs = tuple(line[0] for block in payload['source_scope']
                 for lines in ([cell['lines'] for cell in block['cells']] if 'cells' in block else [block['lines']])
                 for line in lines)
    if refs != tuple(sorted(set(refs))):
        _reject('Wire source aliases must follow the complete native range/cell order.')
    count = len(refs)
    literal_domain = payload.get('source_domain') == _LITERAL_BODY_DOMAIN
    reviewed = set(payload.get('reviewed_body_refs', ())) | set(payload.get('reviewed_partial_refs', ()))
    body_choices = tuple(line[0] for block in payload['source_scope']
                      for lines in ([cell['lines'] for cell in block['cells']] if block['kind'] == 'native_row'
                                    else [block['lines']] if literal_domain and 'literal_body_line' in block else [])
                      for line in lines if line[0] not in reviewed)
    chapter_map = {c['id']: c['id'] for c in payload['chapters']}
    document_map = {d: d for d in payload['document_aliases']}
    if (any(not re.fullmatch(r'c[1-9][0-9]*', c) for c in chapter_map)
            or any(not re.fullmatch(r'd[1-9][0-9]*', d) for d in document_map)):
        _reject('New operation uses only request-local short chapter/document identities.')
    legacy_block = payload.get('block', []) if payload.get('allow_legacy_assignments') else []
    legacy_line_limit = len(_prose_points(legacy_block, document_map, set()))
    visible = Annotated[StrictInt, Field(ge=min(refs, default=1), le=max(refs, default=1),
                                       json_schema_extra={'enum': list(refs)})]
    visible_refs = frozenset(refs)
    refs = Annotated[list[visible], Field(min_length=1, max_length=count)]
    body_count = len(body_choices)
    body_ref = Annotated[StrictInt, Field(ge=min(body_choices, default=1), le=max(body_choices, default=1),
                                        json_schema_extra={'enum': list(body_choices)})]
    if body_choices and (literal_domain or reviewed):
        choices = frozenset(body_choices)

        def allowed(ref):
            if ref in choices:
                return ref
            if ref not in visible_refs:
                raise ValueError('Body ref is outside this pack. Use a supplied, eligible body ref.')
            if ref in reviewed:
                raise ValueError('Reviewed ref cannot be a body ref. Keep its host-owned disposition.')
            raise ValueError(f'Ref {ref} has no certified body location. Remove it from body_refs. '
                             'Keep a pending requirement at this ref as unresolved only in native_dispositions.')
        body_ref = Annotated[body_ref, AfterValidator(allowed)]
    body_refs = Annotated[list[body_ref], Field(min_length=1, max_length=body_count)]
    chapter = Literal.__getitem__(tuple(chapter_map)) if chapter_map else Literal['__none__']
    native = create_model('NativeRequirementAssignmentView', __base__=NativeRequirementAssignment,
        body_refs=(body_refs, ...), chapter_ids=(Annotated[list[chapter], Field(max_length=len(chapter_map))], Field(default_factory=list)),
        context_id=(Annotated[StrictInt, Field(ge=1, le=max(1, count))] | None, None))
    context = create_model('NativeContextView', __base__=NativeContext, refs=(refs, ...),
        parent_id=(Annotated[StrictInt, Field(ge=1, le=max(1, count))] | None, None))
    disposition = create_model('NativeDispositionView', __base__=NativeDisposition, refs=(refs, ...))
    legacy = RequirementAssignment
    if legacy_line_limit:
        points = _legacy_points(legacy_block, document_map)
        document = Literal.__getitem__(tuple(document_map))
        page = Annotated[StrictInt, Field(ge=0, le=max(p[1] for p in points))]
        line = Annotated[StrictInt, Field(ge=0, le=max(p[2] for p in points))]
        window = create_model('BoundedProseWindow', __base__=DirectoryWindow, document_id=(document, ...),
                              page=(page, ...), start_line=(line, ...), end_line=(line, ...))
        legacy = create_model('BoundedProseAssignment', __base__=RequirementAssignment,
            document_id=(document, ...), page=(page, ...), start_line=(line, ...), end_line=(line, ...),
            continuation_windows=(Annotated[list[window], Field(max_length=legacy_line_limit)], Field(default_factory=list)),
            chapter_ids=(Annotated[list[chapter], Field(max_length=len(chapter_map))], Field(default_factory=list)))
    return create_model('NativeRequirementPlanView', __base__=NativeRequirementPlan,
        assignments=(Annotated[list[legacy], Field(max_length=legacy_line_limit)], Field(default_factory=list)),
        contexts=(Annotated[list[context], Field(max_length=count)], Field(default_factory=list)),
        native_assignments=(Annotated[list[native], Field(max_length=body_count)], Field(default_factory=list)),
        native_dispositions=(Annotated[list[disposition], Field(max_length=count)], Field(default_factory=list)))



def _continuous_ranges(ledger, ordinals):
    return all(ledger.ranges[b]['reference']['source_index_ref'] == ledger.ranges[a]['reference']['source_index_ref']
               and ledger.ranges[b]['source_ordinal'] == ledger.ranges[a]['source_ordinal'] + 1
               for a, b in zip(ordinals, ordinals[1:]))


def _source_components(ledger, ordinals):
    groups = []
    for ordinal in ordinals:
        if not groups or not _continuous_ranges(ledger, [groups[-1][-1], ordinal]):
            groups.append([])
        groups[-1].append(ordinal)
    result = []
    for identity, group in enumerate(groups, 1):
        first, last = ledger.ranges[group[0]], ledger.ranges[group[-1]]
        result.append({'id': identity, 'range_ordinals': group, 'document': first['wire']['document'],
                       'open_before': first['source_ordinal'] > 0,
                       'open_after': last['source_ordinal'] + 1 < last['source_range_count']})
    return result


class NativeSourceView:
    def __init__(self, ledger, range_ordinals):
        if (not range_ordinals or any(type(i) is not int for i in range_ordinals)
                or range_ordinals != sorted(set(range_ordinals))
                or range_ordinals[0] < 0 or range_ordinals[-1] >= len(ledger.ranges)):
            _reject('Native packs select ordered complete source ranges, never slice a row.')
        self.ledger, self.range_ordinals = ledger, list(range_ordinals)
        self.ledger_binding = ledger.binding
        self.components = _source_components(ledger, range_ordinals)
        self.component_of = {ordinal: component['id'] for component in self.components
                             for ordinal in component['range_ordinals']}
        selected = set(range_ordinals)
        self.refs = tuple(r for r, a in ledger.atoms.items() if a['range_ordinal'] in selected)
        literal_lines = {}
        visible = set(self.refs)
        for ref in visible.intersection(ledger.literal_body_refs):
            placement = {'literal_body_line': ledger.literal_body_refs[ref][1]}
            previous = ledger.literal_predecessors.get(ref)
            if previous in visible:
                placement['literal_predecessor_ref'] = previous
            literal_lines[ledger.atoms[ref]['range_ordinal']] = placement
        self.chapter_payload = deepcopy(ledger.chapter_payload)
        self.binding = _hash({'source_binding': ledger.binding, 'ranges': range_ordinals,
                              'chapter_aliases': ledger.chapter_map, 'document_aliases': ledger.document_map})
        self.payload = {'operation': REQUIREMENT_OPERATION, 'native_requirement_contract': CONTRACT,
            'source_binding': self.binding, 'source_scope': [
                {**deepcopy(ledger.ranges[i]['wire']), 'component': self.component_of[i],
                 **literal_lines.get(i, {})} for i in range_ordinals],
            'document_aliases': list(ledger.document_map),
            'atom_count': len(self.refs), 'source_closure': 'complete_ranges_with_explicit_components_v1',
            'source_domain': _LITERAL_BODY_DOMAIN if literal_lines else 'grid_body_verified_legacy_prose_v1',
            'source_components': deepcopy(self.components),
            'component_gaps': [{'before': a['id'], 'after': b['id'],
                               'reason': 'document_boundary' if a['document'] != b['document'] else 'unseen_source_ranges'}
                              for a, b in zip(self.components, self.components[1:])],
            'open_before': self.components[0]['open_before'], 'open_after': self.components[-1]['open_after']}
        for key, refs in (('reviewed_body_refs', ledger.reviewed_body_refs),
                          ('reviewed_partial_refs', ledger.reviewed_partial_refs)):
            if selected_refs := visible.intersection(refs):
                self.payload[key] = sorted(selected_refs)


    def response_model(self, legacy_line_limit=0, *, legacy_block=()):
        if self.ledger.binding != self.ledger_binding:
            _reject('This source view belongs to an older ledger binding.')
        if legacy_line_limit != len(_prose_points(legacy_block, self.ledger.document_map, self.ledger.grid_units)):
            _reject('Schema limits must bind the complete supplied legacy prose block.')
        block = deepcopy(legacy_block)
        reverse = {value: key for key, value in self.ledger.document_map.items()}
        for page in block:
            page['document_id'] = reverse.get(page['document_id'], page['document_id'])
        return native_requirement_model({**self.payload, 'chapters': self.chapter_payload,
            'block': block, 'allow_legacy_assignments': bool(legacy_line_limit)})

    def _open_body_edges(self, refs):
        ranges = {self.ledger.atoms[ref]['range_ordinal']: ref for ref in reversed(refs)}
        return [(ranges[ordinal], edge) for component in self.components
                for edge, ordinal in (('open_before', component['range_ordinals'][0]),
                                      ('open_after', component['range_ordinals'][-1]))
                if component[edge] and ordinal in ranges]

    def accept(self, raw, *, legacy_block=(), carried_refs=()):
        line_limit = len(_prose_points(legacy_block, self.ledger.document_map, self.ledger.grid_units))
        value = _parse(self.response_model(line_limit, legacy_block=legacy_block), raw)
        dispositions = {}
        for item in value['native_dispositions']:
            for ref in item['refs']:
                if ref not in self.refs or ref in dispositions:
                    _reject('Every source atom needs exactly one disposition.')
                dispositions[ref] = item['kind']
        missing = [ref for ref in self.refs if ref not in dispositions]
        if missing:
            reviewed_refs = set(self.payload.get('reviewed_body_refs', ()))
            partial_refs = set(self.payload.get('reviewed_partial_refs', ()))
            for kind, refs in (
                ('already_carried', sorted(reviewed_refs.intersection(missing))),
                ('reference', sorted(partial_refs.intersection(missing))),
                ('unresolved', sorted(set(missing) - reviewed_refs - partial_refs)),
            ):
                if not refs:
                    continue
                value['native_dispositions'].append({'refs': refs, 'kind': kind})
                for ref in refs:
                    dispositions[ref] = kind
        def retarget(refs, kind):
            wanted = set(refs)
            if not wanted or all(dispositions.get(ref) == kind for ref in wanted):
                return
            moved = []
            for item in value['native_dispositions']:
                hit = [ref for ref in item['refs'] if ref in wanted]
                item['refs'] = [ref for ref in item['refs'] if ref not in wanted]
                moved.extend(hit)
            value['native_dispositions'] = [item for item in value['native_dispositions'] if item['refs']]
            if moved:
                value['native_dispositions'].append({'refs': sorted(set(moved)), 'kind': kind})
                for ref in moved:
                    dispositions[ref] = kind

        reviewed = self.payload.get('reviewed_body_refs', ())
        partial = self.payload.get('reviewed_partial_refs', ())
        retarget(reviewed, 'already_carried')
        retarget(partial, 'reference')
        context_refs, contexts = set(), []
        for i, group in enumerate(value['contexts'], 1):
            if not set(group['refs']) <= set(self.refs):
                _reject('Context refs must be visible in this source pack.')
            self.ledger.excerpts(group['refs'])
            if group['parent_id'] is not None and not 1 <= group['parent_id'] < i:
                _reject('Context parent must be an earlier declared group; cycles/forward parents are invalid.')
            if context_refs.intersection(group['refs']) or any(dispositions[r] != 'context' for r in group['refs']):
                _reject('Context atoms belong to one shared group, separate from source bodies.')
            context_refs.update(group['refs'])
            contexts.append(group)
        owned, used_contexts, native, cases = set(), set(), [], []
        for item in value['native_assignments']:
            body = item['body_refs']
            if not set(body) <= set(self.refs):
                _reject('Body refs must be visible in this source pack.')
            self.ledger.excerpts(body)
            self.ledger.require_body(body)
            if len({self.component_of[self.ledger.atoms[r]['range_ordinal']] for r in body}) != 1:
                _reject('B cannot join independent source components; keep separate cases for explicit C closure.')
            kinds = {dispositions[r] for r in body}
            if owned.intersection(body) or len(kinds) != 1 or not kinds <= {'requirement', 'external_action', 'unresolved'}:
                _reject('Body atom ownership must be unique and match its source disposition.')
            owned.update(body)
            _scope(item['scope'], item['chapter_ids'])
            if item['scope'] != 'unassigned':
                edges = self._open_body_edges(body)
                if edges or kinds != {'requirement'}:
                    item['scope'] = 'unassigned'
                    item['chapter_ids'] = []
            chain, identity = [], item['context_id']
            while identity is not None:
                if identity > len(contexts):
                    _reject('Select an existing shared source context.')
                used_contexts.add(identity)
                chain.append(contexts[identity - 1])
                identity = contexts[identity - 1]['parent_id']
            source = self.ledger.source(body, list(reversed(chain)), source_join=item['source_join'])
            chapter_ids = [self.ledger.chapter_map[x] for x in item['chapter_ids']]
            record = {'source': source, 'scope': item['scope'], 'chapter_ids': chapter_ids, 'enterprise_confirmed': False}
            native.append(record)
            if item['scope'] == 'unassigned':
                cases.append(self.ledger.case(source, origin=self.binding, candidate_chapter_ids=chapter_ids))
        unused_groups = sorted(set(range(1, len(contexts) + 1)) - used_contexts)
        ungrouped_refs = sorted(r for r, k in dispositions.items() if k == 'context' and r not in context_refs)
        if unused_groups or ungrouped_refs:
            _reject('Every declared context atom/group must reach a real source assignment. '
                    f'Unused group IDs: {unused_groups[:8]}; ungrouped source refs: {ungrouped_refs[:8]}. '
                    'Resolve every such group/ref, including any beyond the listed examples.', path=['contexts'])
        missing_body = sorted(r for r, k in dispositions.items() if k in {'requirement', 'external_action'} and r not in owned)
        if missing_body:
            grouped = {}
            for ref in missing_body:
                component = self.component_of[self.ledger.atoms[ref]['range_ordinal']]
                grouped.setdefault(component, []).append(ref)
            for refs in grouped.values():
                source = self.ledger.source(sorted(refs), [])
                cases.append(self.ledger.case(source, origin=self.binding))
        if not {r for r, k in dispositions.items() if k == 'already_carried'} <= (set(carried_refs) | set(reviewed)):
            _reject('Already-carried source claims need exact host-owned source coverage.')
        guidance = []
        for ordinal in self.range_ordinals:
            for kind in ('unresolved', 'reference', 'evaluator_procedure', 'navigation', 'layout'):
                refs = [r for r, k in dispositions.items() if k == kind and r not in owned and r not in partial
                        and self.ledger.atoms[r]['range_ordinal'] == ordinal]
                if not refs:
                    continue
                source = self.ledger.source(sorted(refs), [])
                if kind == 'unresolved':
                    cases.append(self.ledger.case(source, origin=self.binding))
                else:
                    guidance.append({'kind': kind, 'source': source, 'enterprise_confirmed': False,
                                     'outlet': 'evaluator_reference_only' if kind == 'evaluator_procedure' else 'source_reference_only'})
        # Legacy windows are restored, then validated/placed by the existing prose path.
        assignments = []
        visible = _legacy_points(legacy_block, self.ledger.document_map)
        prose = _prose_points(legacy_block, self.ledger.document_map, self.ledger.grid_units)
        seen, window_count = set(), 0
        for item in value['assignments']:
            _scope(item['scope'], item['chapter_ids'])
            previous = None
            for window in [item, *item.get('continuation_windows', [])]:
                window_count += 1
                start = (window['document_id'], window['page'], window['start_line'])
                end = (window['document_id'], window['page'], window['end_line'])
                if end < start or previous is not None and (start[0] != previous[0] or start <= previous):
                    _reject('Legacy prose windows must move forward within one registered document.')
                selected = {(window['document_id'], window['page'], number)
                            for number in range(window['start_line'], window['end_line'] + 1)}
                if not selected <= visible & prose or seen.intersection(selected):
                    _reject('Legacy prose selects host-verified non-grid lines once; unmapped/grid layout stays pending.')
                seen.update(selected)
                previous = end
            if window_count > line_limit:
                _reject('Legacy prose output exceeded its complete visible-window bound.')
            item = deepcopy(item)
            for window in [item, *item.get('continuation_windows', [])]:
                if window['document_id'] not in self.ledger.document_map:
                    _reject('Use a supplied short document identity for legacy prose.')
                window['document_id'] = self.ledger.document_map[window['document_id']]
            if any(c not in self.ledger.chapter_map for c in item['chapter_ids']):
                _reject('Use a supplied short chapter identity for legacy prose.')
            item['chapter_ids'] = [self.ledger.chapter_map[c] for c in item['chapter_ids']]
            assignments.append(item)
        return {'assignments': assignments, 'native_records': native, 'cases': cases, 'guidance': guidance,
                'source_binding': self.binding, 'complete': not cases,
                'native_dispositions': deepcopy(value['native_dispositions']), 'enterprise_confirmed': False}


class NativeCaseView:
    def __init__(self, ledger, cases):
        self.ledger = ledger
        self.cases = [ledger.validate_case(case) for case in cases]
        if len({case['id'] for case in self.cases}) != len(self.cases):
            _reject('A native pending case may appear only once.')
        self.case_map = {f'q{i}': case for i, case in enumerate(self.cases, 1)}
        pool, by_key, capsules = [], {}, []
        case_refs = {case['id']: [ledger._refs([excerpt]) for excerpt in case['source']['body']['source_excerpts']]
                     for case in self.cases}
        visible = {ref for groups in case_refs.values() for refs in groups for ref in refs}
        for alias, case in self.case_map.items():
            context_ids = []
            for context in case['source']['contexts']:
                key = _hash(context)
                if key not in by_key:
                    by_key[key] = len(pool) + 1
                    pool.append({'id': by_key[key], 'text': context['text'], 'source_join': context['source_join']})
                context_ids.append(by_key[key])
            locations = []
            for refs in case_refs[case['id']]:
                ref = refs[0]
                atom = ledger.atoms[ref]
                location = dict(document=atom['document'], page=atom['page'],
                    unit=ledger.ranges[atom['range_ordinal']]['wire']['unit'],
                    row=atom['row'], column=atom['column'], first_ref=ref, last_ref=refs[-1])
                if ref in ledger.literal_body_refs:
                    location['literal_body_line'] = ledger.literal_body_refs[ref][1]
                    previous = ledger.literal_predecessors.get(ref)
                    if previous in visible:
                        location['literal_predecessor_ref'] = previous
                locations.append(location)
            capsules.append({'id': alias, 'body': case['source']['body']['text'],
                'source_join': case['source']['body']['source_join'], 'context_ids': context_ids,
                'source_locations': locations,
                'source_identity': case['source']['source_id'].removeprefix('native-source-')})
        self.payload = {'operation': CLASSIFICATION_OPERATION,
            'native_requirement_contract': CONTRACT, 'source_binding': _hash({'cases': [c['id'] for c in self.cases], 'chapter_aliases': ledger.chapter_map,
                                       'document_aliases': ledger.document_map}),
            'requirement_cases': capsules, 'native_contexts': pool, 'chapters': deepcopy(ledger.chapter_payload)}

    def accept(self, raw):
        value = _parse(NativeClassificationPlan, raw)
        selected, records = set(), []
        for decision in value['requirement_adjudications']:
            aliases = decision['item_ids']
            if not aliases or len(aliases) != len(set(aliases)) or selected.intersection(aliases) or any(a not in self.case_map for a in aliases):
                _reject('Native C cases must be selected exactly once using supplied short identities.')
            if any(c not in self.ledger.chapter_map for c in decision['chapter_ids']):
                _reject('Use supplied short chapter identities in native C.')
            _scope(decision['scope'], decision['chapter_ids'])
            if decision['scope'] == 'not_applicable' and not decision['basis_windows']:
                _reject('Not-applicable still needs explicit source basis; absence of enterprise data is not a basis.')
            cases = [self.case_map[a] for a in aliases]
            body_refs = []
            contexts, context_keys = [], set()
            for case in cases:
                self.ledger.validate_case(case)
                body_refs.extend(self.ledger._refs(case['source']['body']['source_excerpts']))
                for group in case['source']['contexts']:
                    key = _hash(group)
                    if key not in context_keys:
                        context_keys.add(key)
                        contexts.append({'refs': self.ledger._refs(group['source_excerpts']), 'source_join': group['source_join']})
            if len(body_refs) != len(set(body_refs)):
                _reject('Merged native cases overlap; resolve source ownership before merging.')
            has_literal = any(self.ledger.atoms[r]['base'].get('text_basis') == 'pdf_literal' for r in body_refs)
            if decision['scope'] in {'chapter', 'chapters'} or (has_literal and len(cases) > 1):
                self.ledger.require_body(sorted(body_refs))
            if len(cases) == 1:
                original_join = cases[0]['source']['body']['source_join']
                if decision['source_join'] is not None and decision['source_join'] != original_join:
                    _reject('A single canonical case keeps its existing explicit join; revise its source separately.')
                source = deepcopy(cases[0]['source'])
            else:
                if decision['source_join'] is None:
                    _reject('Merging different native cases requires an explicit inter-case join.')
                if any(case['source']['body']['source_join'] != decision['source_join']
                       and len(case['source']['body']['source_excerpts']) > 1 for case in cases):
                    _reject('Inter-case join cannot rewrite an existing case internal join; retain it for source review.')
                source = self.ledger.source(sorted(body_refs), contexts, source_join=decision['source_join'])
            basis = deepcopy(decision['basis_windows'])
            for window in basis:
                if window['document_id'] not in self.ledger.document_map:
                    _reject('Use a supplied short document identity for C basis.')
                window['document_id'] = self.ledger.document_map[window['document_id']]
            records.append({'item_ids': [c['id'] for c in cases], 'source': source,
                'scope': str(decision['scope']), 'chapter_ids': [self.ledger.chapter_map[c] for c in decision['chapter_ids']],
                'basis_windows': basis, 'enterprise_confirmed': False,
                'outlet': 'manual_source_review' if decision['scope'] == 'unassigned' else
                          'external_action_pending' if decision['scope'] == 'procedure' else 'source_disposition_only'})
            selected.update(aliases)
        for alias in self.case_map:
            if alias in selected:
                continue
            case = self.case_map[alias]
            records.append({'item_ids': [case['id']], 'source': deepcopy(case['source']),
                'scope': 'unassigned', 'chapter_ids': [], 'basis_windows': [], 'enterprise_confirmed': False,
                'outlet': 'manual_source_review'})
            selected.add(alias)
        # Existing C source-window/basis/exclusion validators remain the caller's responsibility.
        items = deepcopy(value['items'])
        for item in items:
            if item['document_id'] not in self.ledger.document_map:
                _reject('Use a supplied short document identity for C classification.')
            item['document_id'] = self.ledger.document_map[item['document_id']]
        return {'items': items, 'native_decisions': records, 'enterprise_confirmed': False}


def native_output_bound(view, *, legacy_block=()):
    """Bound compact ASCII JSON of accepted B replies, with a planning margin.

    Acceptance gives each atom one disposition and permits at most one use in
    either a context group or a body. Nonempty groups have distinct refs, so
    charging a singleton object and a separator per ref covers any grouping.
    Each legacy window consumes a distinct verified prose line. Its charge
    includes the larger primary assignment shape to cover continuation windows.
    """
    atom_count = len(view.refs)
    selectable = set(view.refs) - view.ledger.reviewed_body_refs - view.ledger.reviewed_partial_refs
    grid_refs = {ref for ref in selectable if view.ledger.atoms[ref]['base'].get('text_basis') == 'grid_row'}
    literal_refs = view.ledger.literal_body_refs.keys() & selectable
    body_count = len(grid_refs | literal_refs)
    chapters = list(view.ledger.chapter_map)
    pointer = max(atom_count, 1) if len(str(atom_count)) > len('null') else None
    assignment = {'body_refs': [0], 'context_id': pointer, 'source_join': 'continuation',
                  'scope': 'unassigned', 'chapter_ids': chapters}
    context = {'refs': [0], 'parent_id': pointer, 'source_join': 'continuation'}
    disposition = {'refs': [0], 'kind': 'evaluator_procedure'}
    envelope = {'assignments': [], 'contexts': [], 'native_assignments': [], 'native_dispositions': []}
    assignment_bytes, context_bytes, disposition_bytes = (
        len(_json(item).encode('ascii')) - 1 for item in (assignment, context, disposition))
    ref_bytes_with_separator = sum(len(str(ref)) + 1 for ref in view.refs)
    compact_bytes = (len(_json(envelope).encode('ascii'))
                     + body_count * max(assignment_bytes, context_bytes)
                     + (atom_count - body_count) * context_bytes
                     + atom_count * disposition_bytes
                     + 2 * ref_bytes_with_separator)
    points = _legacy_points(legacy_block, view.ledger.document_map)
    legacy_line_limit = len(_prose_points(legacy_block, view.ledger.document_map, view.ledger.grid_units))
    if legacy_line_limit:
        document = max(view.ledger.document_map, key=len)
        legacy = {'document_id': document, 'page': max(p[1] for p in points),
                  'start_line': max(p[2] for p in points), 'end_line': max(p[2] for p in points),
                  'continuation_windows': [], 'scope': 'unassigned', 'chapter_ids': chapters}
        compact_bytes += legacy_line_limit * (len(_json(legacy).encode('ascii')) + 1)
    margin = 256
    return {'token_upper_bound': compact_bytes + margin, 'compact_json_byte_upper_bound': compact_bytes,
            'planning_margin_tokens': margin, 'native_atoms': atom_count,
            'grid_body_atoms': len(grid_refs), **({'literal_body_atoms': len(literal_refs)} if literal_refs else {}),
            'chapter_count': len(chapters), 'chapter_choice_occurrences': body_count * len(chapters),
            'context_parent_edges': atom_count, 'assignment_context_edges': body_count, 'legacy_window_limit': legacy_line_limit}


def require_native_budget(budget, view, payload, prompt, *, request=None):
    legacy_block = payload.get('block', []) if payload.get('allow_legacy_assignments') else []
    legacy_line_limit = len(_prose_points(legacy_block, view.ledger.document_map, view.ledger.grid_units))
    schema = view.response_model(legacy_line_limit, legacy_block=legacy_block).model_json_schema(mode='validation')
    full_prompt = prompt.rstrip() + '\n\nReturn only JSON matching this schema.\n' + _json(schema)
    wire = {**deepcopy(payload), **view.payload}
    request = request if request is not None else build_request(budget.model, full_prompt, wire, reasoning_effort=budget.reasoning_effort)
    measured = budget.require_request(request, 'directory_requirement')
    output = native_output_bound(view, legacy_block=legacy_block)
    if output['token_upper_bound'] > measured['output_reserve']:
        raise ModelBudgetError('Complete native range scope exceeds its reserved compact B output ceiling.')
    return {**measured, 'worst_accepted_output_tokens': output['token_upper_bound'],
            'output_shape': output, 'schema_sha256': _hash(schema), 'request': request}


def plan_native_packs(ledger, budget, payload_factory, prompt, *, request_factory=None):
    """Find budgeted packs using complete contiguous ranges and measured candidates."""
    views, pending = [], []

    def admit(start, count):
        view = ledger.view(list(range(start, start + count)))
        try:
            payload = payload_factory(view)
            require_native_budget(budget, view, payload, prompt,
                                  request=request_factory(view, payload) if request_factory else None)
        except ModelBudgetError:
            return None
        return view

    start, total = 0, len(ledger.ranges)
    while start < total:
        accepted = admit(start, 1)
        if accepted is None:
            pending.append({'range_ordinals': [start], 'source_binding': ledger.binding,
                'source_reference': deepcopy(ledger.ranges[start]['reference']),
                'range_id': ledger.ranges[start]['base']['range_id'],
                'outlet': 'manual_source_review', 'reason': 'complete_native_record_over_budget',
                'enterprise_confirmed': False})
            start += 1
            continue

        remaining = total - start
        size, rejected = 1, None
        while size < remaining:
            probe = min(size * 2, remaining)
            candidate = admit(start, probe)
            if candidate is None:
                rejected = probe
                break
            size, accepted = probe, candidate

        # Budget may vary across adjacent lengths. Only emit a measured fit and
        # leave every later range for the next pack regardless of the search.
        while rejected is not None and rejected - size > 1:
            probe = (size + rejected) // 2
            candidate = admit(start, probe)
            if candidate is None:
                rejected = probe
            else:
                size, accepted = probe, candidate
        views.append(accepted)
        start += size
    return {'views': views, 'pending_ranges': pending, 'complete': not pending, 'model_calls': 0}
