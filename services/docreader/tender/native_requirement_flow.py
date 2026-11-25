"""Opt-in native requirements: source proof, common C identities and atomic preparation."""
from collections import Counter, defaultdict
from copy import deepcopy
import hashlib
import json
from math import isfinite
from pathlib import Path
from subprocess import run, TimeoutExpired
from tempfile import TemporaryDirectory
from typing import Annotated, Any, Literal, Union, cast
from xml.etree import ElementTree

from pydantic import Field, StrictInt, create_model

from docreader.llm.request import ModelBudgetError
from docreader.tender.directory_adjudication import (
    requirement_cases, validate_requirement_adjudications, apply_requirement_adjudications,
)
from docreader.tender.directory_compile import (
    ClassificationItem, DirectoryPlanRejected, _FrozenLayout, _Source, _compact, _window, _window_text,
    apply_classification,
)
from docreader.tender.directory_sources import SourceRefs
from docreader.tender.native_requirement_sources import (
    CONTRACT, CLASSIFICATION_OPERATION, NativeClassificationPlan, NativeRequirementAdjudication,
    NativeCaseView, _parse, _reject,
)
from docreader.tender.reviewed_source_facts import (
    ReviewedBodyDisposition, apply_reviewed_classification, reviewed_body_summary, reviewed_classification_view,
)
from docreader.tender.schema import Contract


def _json(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':'))


def _hash(value):
    return hashlib.sha256(_json(value).encode()).hexdigest()


def certify_page(lines, owners, grids, physical):
    """A missing table owner is unknown; unique exact outside-box evidence proves prose."""
    def compact(text):
        return ''.join(text.split())
    candidates = {}
    if physical:
        for words in physical['lines']:
            candidates.setdefault(compact(''.join(w[4] for w in words)), []).append(words)
    counts = Counter(compact(line) for line in lines)
    result = []
    for number, text in enumerate(lines):
        owner = owners[number] if number < len(owners) else None
        item = {'line': number, 'text': text, 'source_role': 'unknown'}
        if owner is not None:
            item.update(source_role='grid', unit_id=owner)
        elif not grids:
            item['source_role'] = 'prose'
        elif compact(text) and physical and counts[compact(text)] == 1 and len(candidates.get(compact(text), [])) == 1:
            words = candidates[compact(text)][0]
            intersections, containing = [], []
            for grid in grids:
                loc = grid.get('locator') or {}
                if not all(isinstance(loc.get(k), (int, float)) for k in ('left', 'right', 'top', 'bottom')):
                    intersections.append(grid)
                    continue
                left, right = loc['left'] - 1, loc['right'] + 1
                top, bottom = physical['height'] - loc['top'] - 1, physical['height'] - loc['bottom'] + 1
                if any(x1 < right and left < x2 and y1 < bottom and top < y2 for x1, y1, x2, y2, _ in words):
                    intersections.append(grid)
                if all(left <= x1 <= x2 <= right and top <= y1 <= y2 <= bottom for x1, y1, x2, y2, _ in words):
                    containing.append(grid)
            if not intersections:
                item['source_role'] = 'prose'
            elif len(intersections) == len(containing) == 1:
                item.update(source_role='grid', unit_id=containing[0]['unit_id'])
        result.append(item)
    return result


def physical_pages(artifacts, record, requested):
    data = artifacts.read_bytes(record['original_ref'])
    if not data.startswith(b'%PDF') or not requested:
        return {}
    result = {}
    with TemporaryDirectory(prefix='.native-requirement-', dir=artifacts.root) as directory:
        path = Path(directory) / 'source.pdf'
        path.write_bytes(data)
        for page in sorted(requested):
            completed = run(['pdftotext', '-bbox-layout', '-enc', 'UTF-8', '-f', str(page + 1), '-l', str(page + 1), str(path), '-'],
                            capture_output=True, check=False, timeout=60)
            if completed.returncode:
                continue
            try:
                root = ElementTree.fromstring(completed.stdout)
                element = next(e for e in root.iter() if e.tag.rsplit('}', 1)[-1] == 'page')
                lines = []
                for line in element.iter():
                    if line.tag.rsplit('}', 1)[-1] != 'line':
                        continue
                    words = [[float(w.attrib[k]) for k in ('xMin', 'yMin', 'xMax', 'yMax')] + [w.text or '']
                             for w in line if w.tag.rsplit('}', 1)[-1] == 'word']
                    if words:
                        lines.append(words)
                result[page] = {'height': float(element.attrib['height']), 'lines': lines}
            except (ValueError, ElementTree.ParseError, StopIteration, KeyError):
                continue
    return result


def layout_certificates(ledger, documents):
    proof = {}
    frozen = {d['document_id']: d for d in ledger.documents}
    for document in documents:
        index = ledger.repository.source_index(frozen[document['document_id']]['source_index_ref'])
        grids = {}
        for unit in index.raw['units']:
            if not unit.get('grid'):
                continue
            page = (unit.get('locator') or {}).get('page_ordinal')
            if isinstance(page, int):
                grids.setdefault(page, []).append(unit)
        try:
            physical = physical_pages(ledger.artifacts, frozen[document['document_id']], grids)
        except (OSError, ValueError, TimeoutExpired):
            physical = {}
        for page, text in enumerate(document['pages']):
            owners = document.get('line_units') or []
            proof[document['document_id'], page] = certify_page(text.splitlines(), owners[page] if page < len(owners) else [],
                                                               grids.get(page, []), physical.get(page))
    return proof


def literal_layout_locations(artifacts, index, document):
    """Locate unique whole glyph lines in a same-snapshot physical layout."""
    raw = index.raw
    if (not raw.get('literal_source_ref') or not isinstance(document, _FrozenLayout)
            or document.get('document_id') != index.document['document_id']
            or not document.has_layout_proof(_hash(raw))):
        return {}
    pages = artifacts.read_json(raw['literal_source_ref'])['pages']
    if len(pages) != len(document['pages']):
        return {}

    def bounds(locator):
        box = tuple(locator.get(key) for key in ('left', 'bottom', 'right', 'top'))
        if all(type(value) in (int, float) and isfinite(value) for value in box):
            if box[0] < box[2] and box[1] < box[3]:
                return box

    grids, literals = defaultdict(list), defaultdict(list)
    for unit in raw['units']:
        locator = unit.get('physical_locator') or unit['locator']
        if unit.get('grid'):
            grids[locator.get('page_ordinal')].append(bounds(locator))
        elif unit.get('text_basis') == 'pdf_literal':
            literals[locator['page_ordinal']].append(unit)
    if None in grids:
        return {}
    grid_candidates = {(entry['page_ordinal'], entry['char_index'])
                       for entry in raw['literal_projection_dispositions']
                       if entry['reason'] == 'native_grid_candidate'}
    owners, locations = document.get('line_units') or [], {}
    for page, units in literals.items():
        layout = defaultdict(list)
        page_lines = document['pages'][page].splitlines()
        for number, line in enumerate(page_lines):
            layout[_compact(line)].append(number)
        native_counts = Counter(_compact(unit['text']) for unit in units)
        page_owners = owners[page] if page < len(owners) else []
        for unit in units:
            text = _compact(unit['text'])
            lines = layout.get(text, [])
            if not text or len(lines) != 1 or native_counts[text] != 1:
                continue
            line, = lines
            if page_lines[line].split() != unit['text'].split():
                continue
            if line < len(page_owners) and page_owners[line] is not None:
                continue
            unit_id = unit['unit_id']
            box = bounds(unit['physical_locator'])
            if box is None or any(grid is None or not (
                    box[2] < grid[0] or grid[2] < box[0] or box[3] < grid[1] or grid[3] < box[1])
                    for grid in grids[page]):
                continue
            chars = pages[page]['characters']
            if any((page, i) in grid_candidates or chars[i]['bbox'] is None or
                   {'outside_page', 'invisible_text_bbox_candidate'}.intersection(chars[i]['flags'])
                   for i in raw['literal_units'][unit_id]['char_indices']):
                continue
            locations[unit_id] = {'document_id': document['document_id'], 'page': page, 'line': line}
    return locations


def canonical_text(source):
    return '\n'.join([*(g['text'] for g in source['contexts']), source['body']['text']])


def _case_issue(case):
    return {'code': 'outline_directory_requirement_unassigned', 'native_case_id': case['id'],
            'detail': '精确来源与归属待核：' + case['id'] + '\n' + canonical_text(case['source']) +
                      '\n请对照原件在Word核定完整响应位置；企业事实未确认。\n完整来源绑定：' + _json(case)}


def append_native_records(compiled, records):
    for record in records:
        scope = record['scope']
        if scope == 'unassigned':
            continue
        if scope in {'chapter', 'chapters'}:
            compiled.setdefault('_native_requirements', []).append(deepcopy(record))
        elif scope == 'document':
            compiled['issues'].append({'code': 'outline_directory_document_requirement', 'detail': canonical_text(record['source'])})
        elif scope == 'procedure':
            compiled['issues'].append({'code': 'outline_directory_procedure_requirement',
                'detail': '待执行（不代表已完成）：\n' + canonical_text(record['source'])})
        else:
            compiled['issues'].append({'code': 'outline_directory_requirement_not_applicable',
                'detail': '本次不适用；完整精确来源保留：\n' + canonical_text(record['source']) + '\n依据：\n' + record.get('basis_text', '')})
        compiled.setdefault('_native_requirement_decisions', []).append(deepcopy(record))
        if scope not in {'chapter', 'chapters'}:
            compiled['issues'].append({'code': 'outline_directory_native_source_decision',
                'detail': canonical_text(record['source']) + '\n企业事实未确认；完整来源裁定：' + _json(record)})


def source_windows(ledger, source, documents, proof=None):
    """Conservative legacy-coordinate proxy used only by existing scope/exclusion guards."""
    atoms = [ledger.atoms[r] for r in ledger._refs(source['body']['source_excerpts'])]
    native = defaultdict(set)
    literal = defaultdict(set)
    for atom in atoms:
        key = ledger.document_map[f'd{atom["document"]}'], atom['page']
        unit = atom['source_ref']['unit_id']
        (literal if atom['base'].get('text_basis') == 'pdf_literal' else native)[key].add(unit)
    windows = []
    for document in documents:
        for page, text in enumerate(document['pages']):
            key = document['document_id'], page
            if key not in native and key not in literal:
                continue
            rows = (proof or {}).get(key) or []
            wanted = literal.get(key, set())
            located = {row['unit_id']: row['line'] for row in rows
                       if row.get('source_role') == 'native_literal' and row['unit_id'] in wanted}
            if wanted - located.keys():
                return []
            windows.extend({'document_id': key[0], 'page': page, 'start_line': line, 'end_line': line}
                           for line in sorted(located.values()))
            if key not in native:
                continue
            owners = document.get('line_units') or []
            selected = [row['line'] for row in rows if row.get('unit_id') in native[key]] if rows else [
                line for line, unit in enumerate(owners[page] if page < len(owners) else ()) if unit in native[key]]
            if not selected:
                selected = range(len(text.splitlines()))
            if selected:
                windows.append({'document_id': key[0], 'page': page,
                                'start_line': min(selected), 'end_line': max(selected)})
    return windows


def native_classification_model(payload):
    """Shared basis pool keeps the new C accepted graph bounded by S*K + S + D."""
    s = len(payload['requirement_cases'])
    q = cast(Any, Literal).__getitem__(tuple(c['id'] for c in payload['requirement_cases'])) if s else Literal['__none__']
    chapters = tuple(c['id'] for c in payload['chapters'])
    c = cast(Any, Literal).__getitem__(chapters) if chapters else Literal['__none__']
    ref = Annotated[StrictInt, Field(ge=0, le=max(0, payload['source_line_count'] - 1))]
    span = Annotated[tuple[ref, ref], Field(strict=False)]
    basis = create_model('NativeSharedBasis', __base__=Contract,
                         ranges=(Annotated[list[span], Field(min_length=1, max_length=max(s, 1))], ...))
    decision = create_model('JointNativeRequirementAdjudication', __base__=NativeRequirementAdjudication,
        item_ids=(Annotated[list[q], Field(min_length=1, max_length=max(s, 1))], ...),
        chapter_ids=(Annotated[list[c], Field(max_length=len(chapters))], Field(default_factory=list)),
        basis_windows=(Annotated[list, Field(max_length=0)], Field(default_factory=list)),
        basis_id=(Union[Annotated[StrictInt, Field(ge=1, le=max(1, s))], None], None))
    item = create_model('NativeClassificationEvidence', __base__=Contract,
        kind=(ClassificationItem.model_fields['kind'].annotation, ...), range=(span, ...))
    reviewed = tuple(f['fact_id'] for f in payload.get('reviewed_source_facts', ()))
    reviewed_fields = {}
    if reviewed:
        disposition = create_model('JointReviewedBodyDisposition', __base__=ReviewedBodyDisposition,
            fact_id=(cast(Any, Literal).__getitem__(reviewed), ...), chapter_id=(c | None, None))
        reviewed_fields['reviewed_body_dispositions'] = (
            Annotated[list[disposition], Field(min_length=len(reviewed), max_length=len(reviewed))], ...)
    return create_model('JointNativeClassificationPlan', __base__=NativeClassificationPlan,
        items=(Annotated[list[item], Field(max_length=4 * payload['document_count'])], Field(default_factory=list)),
        basis_groups=(Annotated[list[basis], Field(max_length=s)], Field(default_factory=list)),
        requirement_adjudications=(Annotated[list[decision], Field(max_length=s)], Field(default_factory=list)),
        **reviewed_fields)


class JointClassification:
    def __init__(self, ledger, compiled, documents, *, proof=None, reviewed_body_facts=(),
                 reviewed_body_entry_contract=None):
        self.ledger, self.compiled, self.documents, self.proof = ledger, compiled, documents, proof
        self.reviewed_body_facts = tuple(f for f in reviewed_body_facts if f['kind'] == 'independent_body_response')
        self.reviewed_body_entry_contract = reviewed_body_entry_contract
        classification_view = (reviewed_classification_view(compiled, self.reviewed_body_facts)[0]
                               if self.reviewed_body_facts else compiled)
        self.refs = SourceRefs(documents)
        self.legacy = requirement_cases(classification_view, documents)
        reviewed_points = {(w['document_id'], w['page'], line)
                           for fact in self.reviewed_body_facts for w in fact['source_windows']
                           for line in range(w['start_line'], w['end_line'] + 1)}
        self.reviewed_overlap = {case['id'] for case in self.legacy
                                 if any((w['document_id'], w['page'], line) in reviewed_points
                                        for w in case['source_windows']
                                        for line in range(w['start_line'], w['end_line'] + 1))}
        self.manual_legacy = {c['id'] for c in compiled.get('_requirement_cases', [])
                              if c.get('_geometry_unconfirmed') or c.get('_requirement_pack_failed')}
        self.native = [ledger.validate_case(c) for c in compiled.get('_native_requirement_cases', [])]
        self.native_view = NativeCaseView(ledger, self.native)
        self.native_windows = {case['id']: source_windows(ledger, case['source'], documents, proof)
                               for case in self.native}
        self.all_cases = [*self.legacy, *self.native]
        self.mapping = {f'q{i}': case['id'] for i, case in enumerate(self.all_cases, 1)}
        self.reverse = {value: key for key, value in self.mapping.items()}
        capsules = [{'id': self.reverse[c['id']], 'source_kind': 'legacy', 'source_windows': deepcopy(c['source_windows']),
                     **({'manual_only': True} if c['id'] in self.reviewed_overlap or c['id'] in self.manual_legacy else {})} for c in self.legacy]
        for original, capsule in zip(self.native, self.native_view.payload['requirement_cases']):
            capsules.append({**deepcopy(capsule), 'id': self.reverse[original['id']], 'source_kind': 'native',
                             **({'manual_only': True} if not self.native_windows[original['id']] else {})})
        reading = self.refs.reading()
        reverse_docs = {v: k for k, v in ledger.document_map.items()}
        for doc in reading['documents']:
            doc['document_id'] = reverse_docs[doc['document_id']]
        for capsule in capsules:
            for window in capsule.get('source_windows', []):
                window['document_id'] = reverse_docs[window['document_id']]
        self.payload = {**reading, 'operation': CLASSIFICATION_OPERATION, 'native_requirement_contract': CONTRACT,
            'source_line_count': len(self.refs.lines), 'document_count': len(documents),
            'source_binding': _hash({'native': ledger.binding, 'cases': self.mapping, 'chapters': ledger.chapter_map}),
            'requirement_cases': capsules, 'native_contexts': self.native_view.payload['native_contexts'],
            'chapters': ledger.chapter_payload}
        self.fact_map = {}
        if self.reviewed_body_facts:
            reviewed = reviewed_body_summary(self.reviewed_body_facts)
            for ordinal, fact in enumerate(reviewed, 1):
                alias = f'f{ordinal}'
                self.fact_map[alias] = fact['fact_id']
                fact['fact_id'] = alias
                points = [*fact['source_windows'], *(subject['source'] for subject in fact['subjects'])]
                if fact['entry'] is not None:
                    points.append(fact['entry'])
                for point in points:
                    point['document_id'] = reverse_docs[point['document_id']]
            self.payload['reviewed_source_facts'] = reviewed
            self.payload['source_binding'] = _hash({'joint': self.payload['source_binding'], 'reviewed_facts': self.fact_map,
                                                    'entry_contract': self.reviewed_body_entry_contract})

    def scoped_groups(self):
        """Keep exact shared sources and certified continuations in one scope."""
        cases = self.payload['requirement_cases']
        parents = {case['id']: case['id'] for case in cases}
        owners, continuations = {}, []
        native_sources = {self.reverse[case['id']]: case['source'] for case in self.native}

        def root(alias):
            while parents[alias] != alias:
                parents[alias] = parents[parents[alias]]
                alias = parents[alias]
            return alias

        def link(alias, key):
            parents[root(alias)] = root(owners.setdefault(key, alias))

        for case in cases:
            alias = case['id']
            for window in case.get('source_windows', []):
                for line in range(window['start_line'], window['end_line'] + 1):
                    link(alias, (window['document_id'], window['page'], line))
            if alias not in native_sources:
                continue
            for ref in self.ledger._refs(native_sources[alias]['body']['source_excerpts']):
                link(alias, ref)
                if ref in self.ledger.literal_body_refs:
                    page, line = self.ledger.literal_body_refs[ref]
                    document = f"d{self.ledger.atoms[ref]['document']}"
                    link(alias, (document, page, line))
                previous = self.ledger.literal_predecessors.get(ref)
                if previous is not None:
                    continuations.append((alias, previous))
        for alias, previous in continuations:
            if previous in owners:
                parents[root(alias)] = root(owners[previous])
        groups = defaultdict(list)
        for case in cases:
            if not case.get('manual_only'):
                groups[root(case['id'])].append(case['id'])
        return list(groups.values())

    def scope_payload(self, aliases=None):
        payload = {**self.payload}
        payload['classification_scope'] = 'document_v1' if aliases is None else 'cases_v1'
        selected = set(aliases or ())
        cases = [case for case in payload['requirement_cases'] if case['id'] in selected]
        context_ids = {identity for case in cases for identity in case.get('context_ids', [])}
        payload['requirement_cases'] = cases
        payload['native_contexts'] = [context for context in payload['native_contexts'] if context['id'] in context_ids]
        if aliases is not None:
            pages = {(window['document_id'], window['page']) for case in cases
                     for window in case.get('source_windows', [])}
            pages.update((f"d{loc['document']}", loc['page']) for case in cases for loc in case.get('source_locations', []))
            reading = []
            for document in payload['documents']:
                local = [page for page in document['pages'] if (document['document_id'], page['page']) in pages]
                if local:
                    reading.append(dict(document, pages=local))
            payload['documents'] = reading
            payload['document_count'] = 0
            payload.pop('reviewed_source_facts', None)
        payload['source_binding'] = _hash({'authority': self.payload['source_binding'],
            'scope': payload['classification_scope'], 'cases': [case['id'] for case in cases]})
        return payload

    def accept_scope(self, payload, raw):
        value = _parse(native_classification_model(payload), raw)
        visible = {ref for doc in payload['documents'] for page in doc['pages'] for ref, _ in page['lines']}
        ranges = [span for group in value['basis_groups'] for span in group['ranges']]
        for start, end in [*(item['range'] for item in value['items']), *ranges]:
            self.refs.windows(start, end, visible=visible)
        expected = {case['id'] for case in payload['requirement_cases']}
        selected = [alias for decision in value['requirement_adjudications'] for alias in decision['item_ids']]
        if len(selected) != len(set(selected)) or any(alias not in expected for alias in selected):
            _reject('Cover every case in this scope exactly once.')
        if len(ranges) > len(expected):
            _reject('Shared C basis ranges exceed this scope case budget.')
        self.merge_scopes([value])
        return value

    def merge_scopes(self, scopes):
        """Validate complete coverage against the original authorities before publishing."""
        plan = {'items': [], 'basis_groups': [], 'requirement_adjudications': []}
        selected = set()
        for scope in scopes:
            plan['items'].extend(scope['items'])
            if 'reviewed_body_dispositions' in scope:
                if 'reviewed_body_dispositions' in plan:
                    _reject('Reviewed facts are decided once in the document scope.')
                plan['reviewed_body_dispositions'] = scope['reviewed_body_dispositions']
            offset = len(plan['basis_groups'])
            for decision in scope['requirement_adjudications']:
                basis = decision['basis_id']
                if basis and basis > len(scope['basis_groups']):
                    _reject('C decision references a missing basis group.')
                selected.update(decision['item_ids'])
                plan['requirement_adjudications'].append({**decision, 'basis_id': basis + offset if basis else None})
            plan['basis_groups'].extend(scope['basis_groups'])
        plan['requirement_adjudications'].extend({'item_ids': [alias], 'scope': 'unassigned'}
                                                for alias in self.mapping if alias not in selected)
        if self.fact_map and 'reviewed_body_dispositions' not in plan:
            plan['reviewed_body_dispositions'] = [{'fact_id': alias, 'scope': 'unassigned'} for alias in self.fact_map]
        prepared = self.accept(plan)
        # A singleton still pending needs no new source case or identity.
        prepared['native'] = [record for record in prepared['native']
                              if record['scope'] != 'unassigned' or len(record['item_ids']) > 1]
        return prepared

    def _shadow(self):
        shadow = (reviewed_classification_view(self.compiled, self.reviewed_body_facts)[0]
                  if self.reviewed_body_facts else deepcopy(self.compiled))
        source = _Source(self.documents)
        for case in self.native:
            windows = self.native_windows[case['id']]
            if not windows:
                continue
            text = '\n'.join(_window_text(source, _window(w, source, 'native guard')) for w in windows).strip()
            shadow.setdefault('_requirement_cases', []).append({'id': case['id'], 'source_windows': windows, 'source_text': text})
            shadow['issues'].append({'code': 'outline_directory_requirement_unassigned', 'requirement_case_id': case['id'], 'detail': text})
        return shadow

    def accept(self, raw):
        value = _parse(native_classification_model(self.payload), raw)
        present = {alias for decision in value['requirement_adjudications'] for alias in decision['item_ids']}
        for alias in self.mapping:
            if alias not in present:
                value['requirement_adjudications'].append(
                    {'item_ids': [alias], 'scope': 'unassigned', 'chapter_ids': [], 'basis_id': None})
        if sum(len(g['ranges']) for g in value['basis_groups']) > len(self.mapping):
            _reject('Shared C basis ranges exceed the finite case budget; retain an unassigned manual outlet.')
        bases = [[w for start, end in group['ranges'] for w in self.refs.windows(start, end)] for group in value['basis_groups']]
        source = _Source(self.documents)
        reverse_docs = {v: k for k, v in self.ledger.document_map.items()}
        native_ids = {c['id'] for c in self.native}
        decisions, legacy, native, selected, used = [], [], [], set(), set()
        local_native = {c['id']: f'q{i}' for i, c in enumerate(self.native, 1)}
        for decision in value['requirement_adjudications']:
            ids = [self.mapping[q] for q in decision['item_ids']]
            if selected.intersection(ids) or len(ids) != len(set(ids)):
                _reject('Every common legacy/native case must have exactly one disposition.')
            selected.update(ids)
            is_native = [identity in native_ids for identity in ids]
            if any(is_native) and not all(is_native):
                _reject('Keep mixed source authorities as separate unassigned cases; do not replace legacy source text.')
            identity = decision.pop('basis_id')
            if identity and identity > len(bases):
                _reject('C decision references a missing basis group.')
            basis = bases[identity - 1] if identity else []
            if identity:
                used.add(identity)
            raw_decision = {**decision, 'item_ids': ids, 'chapter_ids': [self.ledger.chapter_map[c] for c in decision['chapter_ids']],
                            'basis_windows': basis}
            undecidable = [i for i in ids if i in self.native_windows and not self.native_windows[i]]
            if undecidable and (decision['scope'] != 'unassigned' or len(ids) != 1):
                _reject('Native source without exact layout ownership stays separately pending for manual review.')
            guard_ids = [i for i in ids if i not in undecidable]
            if guard_ids:
                decisions.append({**{k: v for k, v in raw_decision.items() if k != 'source_join'}, 'item_ids': guard_ids})
            if all(is_native):
                native.append({**decision, 'item_ids': [local_native[x] for x in ids],
                    'basis_windows': [{**w, 'document_id': reverse_docs[w['document_id']]} for w in basis]})
            else:
                if self.reviewed_overlap.intersection(ids) and (decision['scope'] != 'unassigned' or len(ids) != 1):
                    _reject('Legacy source partially overlaps reviewed body authority. Keep the entire case separately pending.')
                if decision.get('source_join') not in (None, 'paragraphs'):
                    _reject('Legacy windows keep their existing source-text semantics.')
                if self.manual_legacy.intersection(ids) and (decision['scope'] != 'unassigned' or len(ids) != 1):
                    _reject('Manual layout review stays separately unassigned until the source is resolved.')
                legacy.append(decisions[-1])
        if selected != set(self.mapping.values()) or used != set(range(1, len(bases) + 1)):
            _reject('Cover every common case once and use every declared shared basis group.')
        try:
            validate_requirement_adjudications(self._shadow(), self.documents, decisions)
        except DirectoryPlanRejected as error:
            _reject(error.detail)
        prepared_native = self.native_view.accept({'items': [], 'requirement_adjudications': native})['native_decisions']
        for record in prepared_native:
            record['basis_text'] = '\n'.join(_window_text(source, _window(w, source, 'native basis')) for w in record['basis_windows']).strip()
        items, kinds = [], set()
        for item in value['items']:
            windows = self.refs.windows(*item['range'])
            key = item['kind'], windows[0]['document_id']
            if key in kinds:
                _reject('Classify each registered document/kind once using complete source evidence.')
            kinds.add(key)
            items.extend({'kind': item['kind'], **window} for window in windows)
        prepared = {'items': items, 'legacy': legacy, 'native': prepared_native}
        if self.reviewed_body_facts:
            prepared['reviewed_body_dispositions'] = [
                {**fact, 'fact_id': self.fact_map[fact['fact_id']],
                 'chapter_id': self.ledger.chapter_map.get(fact['chapter_id'])}
                for fact in value['reviewed_body_dispositions']]
        trial = deepcopy(self.compiled)
        original_rejections = sum(i['code'] == 'outline_directory_classification_rejected' for i in trial['issues'])
        try:
            self._apply_legacy(trial, prepared)
        except DirectoryPlanRejected as error:
            _reject(error.detail)
        if sum(i['code'] == 'outline_directory_classification_rejected' for i in trial['issues']) > original_rejections:
            _reject('Classification evidence must stay inside the registered source.')
        return prepared

    def _apply_legacy(self, compiled, prepared):
        if self.reviewed_body_facts:
            apply_reviewed_classification(compiled, self.documents,
                {'items': prepared['items'], 'requirement_adjudications': prepared['legacy'],
                 'reviewed_body_dispositions': prepared['reviewed_body_dispositions']},
                self.reviewed_body_facts, entry_contract=self.reviewed_body_entry_contract)
        else:
            apply_classification(compiled, self.documents, prepared['items'])
            apply_requirement_adjudications(compiled, self.documents, prepared['legacy'])

    def fallback(self, compiled):
        if self.reviewed_body_facts:
            apply_reviewed_classification(compiled, self.documents, None, self.reviewed_body_facts,
                                          entry_contract=self.reviewed_body_entry_contract)

    def apply(self, compiled, prepared):
        trial = deepcopy(compiled)
        self._apply_legacy(trial, prepared)
        resolved = {identity for record in prepared['native'] for identity in record['item_ids']}
        trial['_native_requirement_cases'] = [c for c in trial.get('_native_requirement_cases', []) if c['id'] not in resolved]
        trial['issues'] = [i for i in trial['issues'] if i.get('native_case_id') not in resolved]
        append_native_records(trial, prepared['native'])
        for record in prepared['native']:
            if record['scope'] == 'unassigned':
                case = self.ledger.case(record['source'], origin='C:' + _hash(record['item_ids']))
                trial['_native_requirement_cases'].append(case)
                trial['issues'].append(_case_issue(case))
        compiled.clear()
        compiled.update(trial)


def classification_output_bound(payload):
    """ASCII-byte ceiling of accepted C graphs, including every S*K edge."""
    s, d = len(payload['requirement_cases']), payload['document_count']
    q = max((c['id'] for c in payload['requirement_cases']), key=len, default='q1')
    chapters = [max((c['id'] for c in payload['chapters']), key=len, default='c1')] * len(payload['chapters'])
    last = max(0, payload['source_line_count'] - 1)
    shape = {'items': [{'kind': 'unresolved', 'range': [last, last]} for _ in range(4 * d)],
             'basis_groups': [{'ranges': [[last, last]]} for _ in range(s)],
             'requirement_adjudications': [{'item_ids': [q], 'scope': 'not_applicable',
                 'chapter_ids': chapters, 'basis_windows': [], 'basis_id': max(s, 1),
                 'source_join': 'continuation'} for _ in range(s)]}
    reviewed = payload.get('reviewed_source_facts', ())
    if reviewed:
        chapter = chapters[0] if chapters and len(_json(chapters[0])) > len('null') else None
        shape['reviewed_body_dispositions'] = [
            {'fact_id': fact['fact_id'], 'scope': 'unassigned', 'chapter_id': chapter} for fact in reviewed]
    return {'token_upper_bound': len(_json(shape).encode('ascii')) + 256,
            **({'reviewed_fact_count': len(reviewed)} if reviewed else {}),
            'case_count': s, 'case_chapter_edges': s * len(chapters), 'shared_basis_ranges': s,
            'classification_items': 4 * d}


def require_classification_budget(budget, request, payload):
    measured = budget.require_request(request, 'directory_classification')
    output = classification_output_bound(payload)
    if output['token_upper_bound'] > measured['output_reserve']:
        raise ModelBudgetError('Complete joint C exceeds its S*K/shared-basis output reserve; preserve every pending source for manual review.')
    return {**measured, 'worst_accepted_output_tokens': output['token_upper_bound'], 'output_shape': output}


def bind_native_targets(compiled, nodes, *, artifacts=None, snapshot=None):
    """One canonical source drives its prose target and persisted guide binding."""
    issues = []
    repository = None
    cache = {}
    if artifacts is not None:
        from docreader.tender.evidence.references import EvidenceRepository
        repository = EvidenceRepository(artifacts)
    for record in compiled.get('_native_requirements', []):
        for chapter_id in record['chapter_ids']:
            node = nodes[chapter_id]
            target_id = f"{node['chapter_id']}:target-{len(node['targets']) + 1}"
            target = {'target_id': target_id, 'kind': 'narrative',
                'purpose': canonical_text(record['source']), 'manual_fill': True,
                'rule_refs': [], 'source_refs': [], 'support_origins': ['tender']}
            if artifacts is not None:
                from docreader.integrations.artifacts import ArtifactIntegrityError
                from docreader.tender.authoring_excerpts import source_excerpts_text
                from docreader.tender.source_body import bind_source_body
                source = record['source']
                groups = [{key: deepcopy(group[key]) for key in ('source_excerpts', 'source_join')}
                          for group in source['contexts']]
                for accepted, group in zip(source['contexts'], groups, strict=True):
                    if accepted['text'] != source_excerpts_text(artifacts, group['source_excerpts'],
                                                              source_join=group['source_join'], repository=repository):
                        raise ArtifactIntegrityError('accepted native context differs from its frozen sources')
                target['source_context_excerpts'] = [excerpt for group in groups for excerpt in group['source_excerpts']]
                parts = [{'source_excerpt': excerpt,
                          'join': source['body']['source_join'] if ordinal else 'paragraphs'}
                         for ordinal, excerpt in enumerate(source['body']['source_excerpts'])]
                target = bind_source_body(artifacts, snapshot, target, parts, context_groups=groups,
                                          repository=repository, cache=cache)
                if target['purpose'] != source['body']['text']:
                    raise ArtifactIntegrityError('accepted native body differs from its frozen sources')
            node['targets'].append(target)
            binding = {'source_id': record['source']['source_id'], 'source': deepcopy(record['source']),
                       'chapter_source_id': chapter_id, 'target_id': target_id,
                       'origin_case_ids': record.get('item_ids', []), 'rendering': 'canonical_prose_once',
                       'enterprise_confirmed': False}
            issues.append({'code': 'outline_directory_native_source_binding',
                'detail': canonical_text(record['source']) + '\n企业事实未确认；对应实际响应位置：' + target_id +
                          '\n完整来源绑定：' + _json(binding)})
    return issues


class JointRequirements:
    """One joint B inventory: complete ranges and a unique prose owner per page."""
    def __init__(self, ledger, documents, applicability, identity=None, *, carried_points=(), reviewed_body_facts=()):
        self.ledger, self.documents = ledger, documents
        self.carried_points = set(carried_points)
        self.proof = layout_certificates(ledger, documents)
        for fact in reviewed_body_facts:
            for window in fact['source_windows']:
                page = self.proof[window['document_id'], window['page']]
                for line in range(window['start_line'], window['end_line'] + 1):
                    page[line]['source_role'] = 'reviewed_body'

        locations, continuation_edges = {}, []
        literal_ranges = {(ledger.document_map[f'd{entry["wire"]["document"]}'], entry['reference']['unit_id']): entry
                          for entry in ledger.ranges if entry['base'].get('text_basis') == 'pdf_literal'}
        frozen = {doc['document_id']: doc for doc in ledger.documents}
        for document in documents:
            doc = document['document_id']
            index = ledger.repository.source_index(frozen[doc]['source_index_ref'])
            for unit, point in literal_layout_locations(ledger.artifacts, index, document).items():
                page, line = point['page'], point['line']
                row = self.proof[doc, page][line]
                if ((doc, unit) in literal_ranges and row['source_role'] == 'prose'
                        and (doc, page, line) not in carried_points):
                    row.update(source_role='native_literal', unit_id=unit)
                    locations[doc, unit] = (page, line)
            previous = None
            for page in range(len(document['pages'])):
                for row in self.proof[doc, page]:
                    if not row['text'].strip():
                        continue
                    current = (doc, row['unit_id']) if row['source_role'] == 'native_literal' else None
                    if previous and current and locations[current][0] - locations[previous][0] <= 1:
                        before, after = literal_ranges[previous], literal_ranges[current]
                        if (after['ordinal'] == before['ordinal'] + 1
                                and after['source_ordinal'] == before['source_ordinal'] + 1):
                            continuation_edges.append((previous, current))
                    previous = current
        ledger.bind_literal_locations(locations, continuation_edges=continuation_edges)
        ledger.bind_reviewed_body_refs(excerpt for fact in reviewed_body_facts for excerpt in fact['source_excerpts'])
        self.identity = {k: identity.get(k) for k in ('source_version', 'domain_revision')} if identity else {}
        self.applicability = deepcopy(applicability)
        self.page_owner = {}
        for ordinal, source in enumerate(ledger.ranges):
            wire = source['wire']
            key = ledger.document_map[f'd{wire["document"]}'], wire['page']
            self.page_owner.setdefault(key, ordinal)
        for document in documents:
            candidates = [(page, ordinal) for (doc, page), ordinal in self.page_owner.items() if doc == document['document_id']]
            for page in range(len(document['pages'])):
                key = document['document_id'], page
                if key not in self.page_owner and candidates:
                    self.page_owner[key] = min(candidates, key=lambda pair: (abs(pair[0] - page), pair[0]))[1]

    def block(self, view, *, aliases=False):
        visible = {(self.ledger.document_map[f'd{self.ledger.ranges[i]["wire"]["document"]}'],
                    self.ledger.ranges[i]['wire']['page']) for i in view.range_ordinals}
        visible.update(key for key, ordinal in self.page_owner.items() if ordinal in view.range_ordinals)
        reverse = {v: k for k, v in self.ledger.document_map.items()}
        block = []
        for document in self.documents:
            for page in range(len(document['pages'])):
                key = document['document_id'], page
                if key not in visible:
                    continue
                lines = deepcopy(self.proof[key])
                if self.page_owner[key] not in view.range_ordinals:
                    for line in lines:
                        if line['source_role'] == 'prose':
                            line['source_role'] = 'context'
                block.append({'document_id': reverse[key[0]] if aliases else key[0], 'page': page, 'lines': lines})
        return block

    def payload(self, view):
        return {**view.payload, **self.identity, 'chapters': view.chapter_payload,
                'applicability': self.applicability, 'block': self.block(view, aliases=True),
                'allow_legacy_assignments': True}

    def accept(self, view, raw, compiled):
        block = self.block(view)
        prepared = view.accept(raw, legacy_block=block)
        from docreader.tender.directory_compile import validate_requirement_windows
        from docreader.tender.directory_choices import validate_requirement_exclusions
        try:
            validate_requirement_windows(self.documents, prepared['assignments'], block)
            validate_requirement_exclusions(compiled, prepared['assignments'])
            guards = []
            for record in prepared['native_records']:
                windows = source_windows(self.ledger, record['source'], self.documents, self.proof)
                if not windows and record['scope'] != 'unassigned':
                    _reject('Native source without exact layout ownership stays pending for manual review.')
                guards.extend({**w, 'scope': record['scope'], 'chapter_ids': record['chapter_ids']} for w in windows)
            validate_requirement_exclusions(compiled, guards)
            covers = {c['id'] for c in compiled['chapters'] if c.get('presentation') == 'cover'}
            if any(covers.intersection(r['chapter_ids']) for r in prepared['native_records']):
                _reject('Cover is not a native chapter response target.')
        except DirectoryPlanRejected as error:
            _reject(error.detail)
        return prepared

    def apply(self, compiled, prepared):
        from docreader.tender.directory_compile import apply_requirements
        trial = deepcopy(compiled)
        apply_requirements(trial, self.documents, prepared['assignments'])
        append_native_records(trial, prepared['native_records'])
        trial.setdefault('_native_requirement_cases', []).extend(prepared['cases'])
        trial['issues'].extend(_case_issue(case) for case in prepared['cases'])
        for record in prepared['guidance']:
            trial['issues'].append({'code': 'outline_directory_native_source_reference',
                'detail': record['outlet'] + '\n' + canonical_text(record['source']) + '\n完整来源：' + _json(record)})
        compiled.clear()
        compiled.update(trial)

    def pending(self, compiled, ordinals, reason):
        failed = set(ordinals)
        reviewed = self.ledger.reviewed_body_refs | self.ledger.reviewed_partial_refs
        for ordinal in ordinals:
            refs = [r for r, a in self.ledger.atoms.items() if a['range_ordinal'] == ordinal and r not in reviewed]
            if not refs:
                continue
            case = self.ledger.case(self.ledger.source(refs, []), origin='manual:' + reason)
            compiled.setdefault('_native_requirement_cases', []).append(case)
            compiled['issues'].append(_case_issue(case))
        for (document, page), owner in self.page_owner.items():
            if owner in failed:
                self._pending_layout(compiled, document, page, 'prose', '_requirement_pack_failed')

    def reviewed_partial_geometry(self, compiled):
        for ref in sorted(self.ledger.reviewed_partial_refs):
            atom = self.ledger.atoms[ref]
            source = self.ledger.source([ref], [])
            case = self.ledger.case(source, origin='manual:reviewed_partial_atom')
            evidence_ref = self.ledger.artifacts.put_json(case)
            document = self.ledger.document_map[f'd{atom["document"]}']
            location = f'{document} 第{atom["page"] + 1}页'
            if atom['row'] is not None and atom['column'] is not None:
                location += f'，原表第{atom["row"] + 1}行第{atom["column"] + 1}列'
            else:
                location += f'，原文字符区间 {atom["start"] + 1}–{atom["end"]}'
            compiled['issues'].append({'code': 'outline_directory_requirement_unassigned',
                'native_case_id': case['id'], 'source_evidence_ref': evidence_ref,
                'detail': f'{location}与已核定正文部分重叠，请核对其余内容是否另需应答。\n' + source['body']['text']})

    def _pending_layout(self, compiled, document, page, role, flag):
        from docreader.tender.directory_compile import apply_requirements
        groups = []
        for row in self.proof[document, page]:
            line = row['line']
            if (row['source_role'] != role or not row['text'].strip()
                    or (document, page, line) in self.carried_points):
                continue
            if not groups or line != groups[-1][-1] + 1:
                groups.append([])
            groups[-1].append(line)
        for group in groups:
            before = len(compiled['issues'])
            apply_requirements(compiled, self.documents, [{'document_id': document, 'page': page,
                'start_line': group[0], 'end_line': group[-1], 'scope': 'unassigned'}])
            if len(compiled['issues']) == before:
                continue
            issue = compiled['issues'][-1]
            for case in compiled.get('_requirement_cases', []):
                if case['id'] == issue.get('requirement_case_id'):
                    case[flag] = True
                    if flag == '_requirement_pack_failed':
                        issue['_keep_pending_detail'] = True
                        issue['detail'] = (f'{document} 第{page + 1}页第{group[0] + 1}–{group[-1] + 1}行，'
                                           '要求提取未完成，请对照原件核定。\n' + case['source_text'])
                    break

    def unknown_geometry(self, compiled):
        for document, page in self.proof:
            self._pending_layout(compiled, document, page, 'unknown', '_geometry_unconfirmed')
