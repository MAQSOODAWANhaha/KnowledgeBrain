"""Exact read-only source selections; native template rendering stays separate."""
from __future__ import annotations

from copy import deepcopy
import hashlib
import json

from docreader.integrations.artifacts import ArtifactIntegrityError
from docreader.llm.request import ModelContractError
from docreader.tender.evidence.references import EvidenceRepository, fragment, selector
from docreader.tender.schema import SourceExcerptRef, SourceUnitRef


def source_excerpt_key(reference):
    value = SourceExcerptRef.model_validate(reference).model_dump(exclude_unset=True)
    return (value['source_ref']['source_index_ref']['sha256'], value['source_ref']['unit_id'],
            tuple((s['source_range_id'], s['start'], s['end']) for s in value['selectors']))


def response_row_id(target_id, excerpts, rule_refs):
    content = {'sources': [source_excerpt_key(ref) for ref in excerpts], 'rules': rule_refs}
    encoded = json.dumps(content, ensure_ascii=False, sort_keys=True, separators=(',', ':'))
    return target_id + ':row-' + hashlib.sha256(encoded.encode()).hexdigest()[:24]


def bind_source_excerpts(context, range_ids, *, path=()):
    """Resolve visible range aliases without using the template-to-grid mapping."""
    result, seen = [], set()
    mapping = getattr(context, 'excerpts', {})
    for ordinal, identity in enumerate(range_ids):
        if identity not in mapping:
            raise ModelContractError([{'path': [*path, ordinal], 'code': 'outline_unknown_reference',
                                       'detail': 'Select exact sources supplied in this request.'}])
        for reference in mapping[identity]:
            key = source_excerpt_key(reference)
            if key not in seen:
                seen.add(key)
                result.append(deepcopy(reference))
    return result


def load_source_excerpt(artifacts, reference, *, repository=None):
    """Materialize selected text, never crop or return its native template grid."""
    value = SourceExcerptRef.model_validate(reference).model_dump(exclude_unset=True)
    repository = repository if repository is not None else EvidenceRepository(artifacts)
    index = repository.source_index(value['source_ref']['source_index_ref'])
    unit_id = value['source_ref']['unit_id']
    native = next((unit for unit in index.raw['units'] if unit['unit_id'] == unit_id), None)
    if native is None:
        raise ArtifactIntegrityError('source excerpt refers to an unknown frozen unit')
    ranges = []
    for selected in value['selectors']:
        base = index.by_id.get(selected['source_range_id'])
        if base is None or base['unit_id'] != unit_id:
            raise ArtifactIntegrityError('source excerpt selector belongs to another native unit')
        if selected['start'] == selected['end'] and base['start'] != base['end']:
            raise ArtifactIntegrityError('source excerpt cannot replace a nonempty range with an empty slice')
        materialized = index.materialize(selected)
        whole = (selected['range_id'] == base['range_id'] and
                 (selected['start'], selected['end']) == (base['start'], base['end']))
        expected = fragment(base, selected['start'], selected['end'])['range_id']
        if not whole and selected['range_id'] != expected:
            raise ArtifactIntegrityError('source excerpt identity does not match its frozen interval')
        ranges.append(materialized)
    text = '\n'.join(part['text'] for part in ranges)
    unit = {key: deepcopy(item) for key, item in native.items() if key not in {'text', 'grid'}}
    unit['text'] = text
    return {'document': deepcopy(index.document), 'unit': unit, 'text': text, 'grid': None, 'ranges': ranges}


def whole_source_excerpts(artifacts, reference, *, repository=None):
    """Expand an explicitly selected whole unit into its real native range owners."""
    value = SourceUnitRef.model_validate(reference).model_dump()
    repository = repository if repository is not None else EvidenceRepository(artifacts)
    index = repository.source_index(value['source_index_ref'])
    if not any(unit['unit_id'] == value['unit_id'] for unit in index.raw['units']):
        raise ArtifactIntegrityError('source excerpt refers to an unknown frozen unit')
    selected = [row for row in index.ranges if row['unit_id'] == value['unit_id']]
    if not selected:
        # Spreadsheet row units can belong to several side-by-side native grids.
        selected = [row for row in index.ranges if value['unit_id'] in (
            row.get('table_ids') or [row.get('table_id')])]
    result = []
    for row in selected:
        native_ref = {'source_index_ref': value['source_index_ref'], 'unit_id': row['unit_id']}
        if result and result[-1]['source_ref'] == native_ref:
            result[-1]['selectors'].append(selector(row))
        else:
            result.append({'source_ref': native_ref, 'selectors': [selector(row)]})
    if not result:
        raise ArtifactIntegrityError('whole source has no frozen native ranges')
    return result
