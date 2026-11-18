"""Compact semantic selections decoded into the existing DirectoryPlan compiler."""

import hashlib
import json
from typing import Annotated

from pydantic import Field, ValidationError

from docreader.llm.request import ModelContractError
from docreader.tender.directory_compile import WireDepth, WirePresentation
from docreader.tender.schema import Contract, NonnegativeInt, ResponseTargetKind, Text


LEGACY_DIRECTORY_REF_CONTRACT = 'source_refs_compact_plan_v11'
DIRECTORY_REF_CONTRACT = 'source_refs_compact_plan_v12'
RefRange = Annotated[list[NonnegativeInt], Field(min_length=2, max_length=2)]


class RefSubject(Contract):
    source: RefRange
    role: Text


class RefNode(Contract):
    heading_ref: NonnegativeInt
    body_ranges: list[RefRange]
    response_kind: ResponseTargetKind
    subjects: list[RefSubject] = Field(default_factory=list)
    content_start_ref: NonnegativeInt | None = None
    parent_ref: NonnegativeInt | None = None
    name: Text | None = None


class RefEntry(Contract):
    name_ref: NonnegativeInt
    excluded_by: list[RefRange] = Field(default_factory=list)


class RefComposition(Contract):
    priority_basis: list[RefRange] = Field(default_factory=list)
    entries: list[RefEntry] = Field(default_factory=list)


class RefItem(Contract):
    node_ref: NonnegativeInt | None = None
    entry_ref: NonnegativeInt | None = None
    depth: WireDepth
    presentation: WirePresentation
    condition: RefRange | None = None
    clause: list[RefRange] = Field(default_factory=list)


class RefScope(Contract):
    title_ref: NonnegativeInt | None = None
    items: list[RefItem] = Field(default_factory=list)


class RefExclusion(Contract):
    node_ref: NonnegativeInt
    source: RefRange


class RefBinding(Contract):
    source: RefRange
    response_node_ref: NonnegativeInt
    reference_node_refs: list[NonnegativeInt] = Field(default_factory=list)


class RefDirectoryPlan(Contract):
    nodes: list[RefNode] = Field(default_factory=list)
    composition: RefComposition = Field(default_factory=RefComposition)
    scopes: list[RefScope] = Field(default_factory=list)
    excluded: list[RefExclusion] = Field(default_factory=list)
    source_only: list[NonnegativeInt] = Field(default_factory=list)
    format_bindings: list[RefBinding] = Field(default_factory=list)


class RefDirectoryPlanV2(RefDirectoryPlan):
    reading_only_refs: list[NonnegativeInt] = Field(default_factory=list)


def _identity(kind, value):
    encoded = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':'))
    return kind + '-' + hashlib.sha256(encoded.encode()).hexdigest()


def _issue(detail, path=(), reference=None):
    issue = {'path': list(path), 'code': 'directory_ref_plan_invalid', 'detail': detail}
    if reference is not None:
        issue['ref'] = reference
    return issue


def _rejected(detail, path=(), reference=None):
    return ModelContractError([_issue(detail, path, reference)])


def decode_plan(raw, source, *, visible=None, known_node_refs=None, model=RefDirectoryPlan):
    try:
        plan = model.model_validate(raw).model_dump()
    except ValidationError as error:
        raise ModelContractError([{'path': list(item['loc']), 'code': item['type'], 'detail': item['msg']}
                                  for item in error.errors(include_input=False)]) from None
    refs = [node['heading_ref'] for node in plan['nodes']]
    known = set(refs)
    candidates = known if known_node_refs is None else set(known_node_refs)
    issues, seen = [], set()
    for index, reference in enumerate(refs):
        if reference in seen or reference not in candidates:
            issues.append(_issue('node refs must be unique supplied candidates',
                                 ['nodes', index, 'heading_ref'], reference))
        seen.add(reference)

    selections = [(['nodes', index, 'parent_ref'], node['parent_ref'])
                  for index, node in enumerate(plan['nodes'])]
    selections.extend((['scopes', scope_index, 'items', item_index, 'node_ref'], item['node_ref'])
                      for scope_index, scope in enumerate(plan['scopes'])
                      for item_index, item in enumerate(scope['items']))
    selections.extend((['excluded', index, 'node_ref'], item['node_ref'])
                      for index, item in enumerate(plan['excluded']))
    selections.extend((['source_only', index], reference) for index, reference in enumerate(plan['source_only']))
    for index, binding in enumerate(plan['format_bindings']):
        selections.append((['format_bindings', index, 'response_node_ref'], binding['response_node_ref']))
        selections.extend((['format_bindings', index, 'reference_node_refs', offset], reference)
                          for offset, reference in enumerate(binding['reference_node_refs']))
    reading_enabled = issubclass(model, RefDirectoryPlanV2)
    for path, reference in selections:
        if reference is not None and reference not in known:
            detail = f'node ref {reference} must refer to a declared node; declare or select a real material node'
            if reading_enabled:
                detail += '; pure reading navigation belongs in reading_only_refs'
            issues.append(_issue(detail, path, reference))
    if issues:
        raise ModelContractError(issues)

    def at(path, reference, select):
        try:
            return select()
        except ModelContractError as error:
            raise ModelContractError([{**item, 'path': [*path, *item['path']], 'ref': reference}
                                      for item in error.errors]) from None

    def point(reference, path, *, nonempty=False):
        return at(path, reference, lambda: source.point(reference, visible=visible, nonempty=nonempty))

    node_ids = {reference: _identity('source-node', point(reference, ['nodes', index, 'heading_ref'], nonempty=True))
                for index, reference in enumerate(refs)}

    def node_id(reference):
        return node_ids.get(reference)

    def windows(ranges, path):
        return [window for index, (start, end) in enumerate(ranges)
                for window in at([*path, index], [start, end], lambda: source.windows(start, end, visible=visible))]

    def single(span, path):
        if span is None:
            return None
        selected = at(path, span, lambda: source.windows(*span, visible=visible))
        if len(selected) != 1:
            raise _rejected('this source selection requires one physical-page window; use separate clause ranges', path, span)
        return selected[0]

    entries = [entry['name_ref'] for entry in plan['composition']['entries']]
    issues, seen = [], set()
    for index, reference in enumerate(entries):
        if reference in seen:
            issues.append(_issue('composition source refs must be unique', ['composition', 'entries', index, 'name_ref'], reference))
        seen.add(reference)
    entry_ids = {reference: number for number, reference in enumerate(entries)}
    for scope_index, scope in enumerate(plan['scopes']):
        for item_index, raw in enumerate(scope['items']):
            if raw['entry_ref'] is not None and raw['entry_ref'] not in entry_ids:
                issues.append(_issue('entry selection must refer to the supplied composition',
                                     ['scopes', scope_index, 'items', item_index, 'entry_ref'], raw['entry_ref']))
    if issues:
        raise ModelContractError(issues)

    def item(raw, path):
        entry = raw['entry_ref']
        return {'node_id': node_id(raw['node_ref']), 'entry': entry_ids.get(entry),
                'depth': raw['depth'], 'presentation': raw['presentation'],
                'condition': single(raw['condition'], [*path, 'condition']),
                'clause': windows(raw['clause'], [*path, 'clause'])}

    nodes = []
    for index, node in enumerate(plan['nodes']):
        path = ['nodes', index]
        heading = point(node['heading_ref'], [*path, 'heading_ref'], nonempty=True)
        start = node['content_start_ref']
        nodes.append({'id': node_id(node['heading_ref']), 'heading': heading,
                      'body_windows': windows(node['body_ranges'], [*path, 'body_ranges']), 'response_kind': node['response_kind'],
                      'subjects': [{'source': single(subject['source'], [*path, 'subjects', offset, 'source']), 'role': subject['role']}
                                   for offset, subject in enumerate(node['subjects'])],
                      'content_start': point(start, [*path, 'content_start_ref']) if start is not None else None,
                      'parent_id': node_id(node['parent_ref']), 'name': node['name']})
    bindings = []
    for index, raw in enumerate(plan['format_bindings']):
        binding = {'source': single(raw['source'], ['format_bindings', index, 'source']), 'response_node_id': node_id(raw['response_node_ref']),
                   'reference_node_ids': [node_id(reference) for reference in raw['reference_node_refs']]}
        bindings.append({'id': _identity('source-binding', binding), **binding})
    decoded = {'nodes': nodes, 'composition': {'priority_basis': windows(plan['composition']['priority_basis'], ['composition', 'priority_basis']),
            'entries': [{'name': point(entry['name_ref'], ['composition', 'entries', index, 'name_ref'], nonempty=True),
                         'excluded_by': windows(entry['excluded_by'], ['composition', 'entries', index, 'excluded_by'])}
                        for index, entry in enumerate(plan['composition']['entries'])]},
            'scopes': [{'title': point(scope['title_ref'], ['scopes', index, 'title_ref'], nonempty=True)
                        if scope['title_ref'] is not None else None,
                        'items': [item(raw, ['scopes', index, 'items', offset]) for offset, raw in enumerate(scope['items'])]}
                       for index, scope in enumerate(plan['scopes'])],
            'excluded': [{'node_id': node_id(raw['node_ref']), 'source': single(raw['source'], ['excluded', index, 'source'])}
                         for index, raw in enumerate(plan['excluded'])],
            'source_only': [node_id(reference) for reference in plan['source_only']], 'format_bindings': bindings}
    if reading_enabled:
        reading, seen = [], set()
        for index, reference in enumerate(plan['reading_only_refs']):
            path = ['reading_only_refs', index]
            if reference in seen or reference in known:
                raise _rejected('reading refs must be unique and cannot select a declared node', path, reference)
            seen.add(reference)
            reading.append(point(reference, path, nonempty=True))
        decoded['reading_only_sources'] = reading
    return decoded
