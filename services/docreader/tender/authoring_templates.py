"""Host-owned exact template selections over frozen native source units."""
from __future__ import annotations

from copy import deepcopy
import hashlib
import json

from docreader.integrations.artifacts import ArtifactIntegrityError
from docreader.tender.evidence.references import EvidenceRepository, fragment, selector
from docreader.tender.schema import TemplatePartRef


def template_part_key(reference):
    part = TemplatePartRef.model_validate(reference).model_dump(exclude_unset=True)
    return (part['source_ref']['source_index_ref']['sha256'], part['source_ref']['unit_id'],
            tuple((s['source_range_id'], s['start'], s['end']) for s in part['selectors']))


def load_template_part(artifacts, reference, *, repository=None):
    """Read only the selected text, or an explicitly selected complete native grid/image."""
    from docreader.tender.authoring import load_outline_source

    repository = repository if repository is not None else EvidenceRepository(artifacts)
    part = TemplatePartRef.model_validate(reference).model_dump(exclude_unset=True)
    loaded = load_outline_source(artifacts, part['source_ref'], repository=repository)
    unit = loaded['unit']
    if part['selectors']:
        if unit.get('grid') or loaded.get('image_ref') or unit['kind'] == 'table_row':
            raise ArtifactIntegrityError('template selectors cannot crop native grids or images')
        index = repository.source_index(part['source_ref']['source_index_ref'])
        texts = []
        for selected in part['selectors']:
            base = index.by_id.get(selected['source_range_id'])
            if base is None or base['unit_id'] != unit['unit_id']:
                raise ArtifactIntegrityError('template selector belongs to another native unit')
            if selected['start'] >= selected['end']:
                raise ArtifactIntegrityError('template text selection must not be empty')
            materialized = index.materialize(selected)
            expected = fragment(base, selected['start'], selected['end'])['range_id']
            whole_range = (selected['range_id'] == base['range_id'] and
                           (selected['start'], selected['end']) == (base['start'], base['end']))
            if not whole_range and selected['range_id'] != expected:
                raise ArtifactIntegrityError('template selector identity does not match its frozen interval')
            texts.append(materialized['text'])
        unit['text'] = '\n'.join(texts)
    if unit.get('grid'):
        unit['text'] = ''  # The grid already contains its fixed cell text.
    text = unit.get('text', '')
    ocr_text = (loaded.get('ocr') or {}).get('text', '')
    if ocr_text.strip() and ocr_text != text:
        text = '\n'.join(value for value in (text, ocr_text) if value)
    loaded.update(text=text, grid=unit.get('grid'))
    return loaded


def prepare_template_parts(artifacts, context, source_records=None):
    """Expose selectable paragraph/whole-grid anchors without repeating original prose.

    Returns (records, short-view); the caller batches records indivisibly, exposes
    only parts whose range_ids were sent, and maps stable part identities locally.
    """
    repository = context.repository
    try:
        frozen = artifacts.read_json(context.input['frozen_ref']) if context.input.get('frozen_ref') else {}
    except (OSError, ValueError, KeyError):
        context.template_parts = {}
        return list(source_records or []), []
    native, grids = {}, {}
    for document in frozen.get('documents', []):
        try:
            source_ref = document['source_index_ref']
            index = repository.source_index(source_ref)
        except (OSError, ValueError, KeyError, TypeError):
            continue  # Context already reports unavailable frozen sources.
        units = {u['unit_id']: u for u in index.raw['units']}
        for row in index.ranges:
            native[row['range_id']] = (source_ref, row, document)
            for ref in context.sources.get(row['range_id'], []):
                if units.get(ref['unit_id'], {}).get('grid'):
                    key = (source_ref['sha256'], ref['unit_id'])
                    grids.setdefault(key, {'source_ref': ref, 'range_ids': []})['range_ids'].append(row['range_id'])
    if source_records is None:
        source_records = [{'source': {'range_id': row['range_id'], 'text': row['text'],
            'heading_path': row.get('heading_path', ''), 'kind': row['kind'], 'role': document['role']},
            'rules': []} for _, row, document in native.values()]
    context.template_parts = {}
    parts, records = [], []

    def expose(source_ref, selected, range_ids):
        part = {'source_ref': deepcopy(source_ref), 'selectors': selected}
        encoded = json.dumps(template_part_key(part), separators=(',', ':'), ensure_ascii=False)
        identity = 'part-' + hashlib.sha256(encoded.encode()).hexdigest()[:24]
        if identity not in context.template_parts:
            context.template_parts[identity] = part
            parts.append({'part_id': identity, 'range_ids': range_ids})

    for original in source_records:
        sources = original.get('sources', [original['source']] if 'source' in original else [])
        if len(sources) != 1:
            records.append(deepcopy(original))
            continue
        view = sources[0]
        source = native.get(view['range_id'])
        if source is None:
            records.append(deepcopy(original))
            continue
        source_index_ref, row, _ = source
        references = context.sources.get(row['range_id'], [])
        if any((ref['source_index_ref']['sha256'], ref['unit_id']) in grids for ref in references):
            records.append(deepcopy(original))
            continue
        if row['kind'] == 'image_region':
            records.append(deepcopy(original))
            if references:
                expose(references[0], [], [row['range_id']])
            continue
        if row['kind'] != 'section' or not row['text'].strip('\n') or view.get('text') != row['text']:
            records.append(deepcopy(original))
            continue
        offset, first = row['start'], True
        for paragraph in row['text'].split('\n'):
            end = offset + len(paragraph)
            if paragraph:
                piece = fragment(row, offset, end)
                identity = piece['range_id']
                context.sources[identity] = deepcopy(references)
                context.range_roots[identity] = row['source_range_id']
                context.excerpts[identity] = [{'source_ref': {
                    'source_index_ref': source_index_ref, 'unit_id': row['unit_id']},
                    'selectors': [selector(piece)]}]
                records.append({**{key: deepcopy(value) for key, value in original.items()
                                   if key not in {'source', 'sources', 'rules'}},
                    'source': {**deepcopy(view), 'range_id': identity, 'text': paragraph},
                    'rules': deepcopy(original.get('rules', [])) if first else []})
                first = False
                expose({'source_index_ref': source_index_ref, 'unit_id': row['unit_id']},
                       [selector(piece)], [identity])
            offset = end + 1

    # Rows sharing a grid are one indivisible record. Connected grids on the same
    # spreadsheet row share a record too, so their physical row text is sent once.
    parents = list(range(len(records)))

    def root(number):
        while parents[number] != number:
            parents[number] = parents[parents[number]]
            number = parents[number]
        return number

    seen = {}
    visible = set()
    for number, record in enumerate(records):
        for view in record.get('sources', [record['source']] if 'source' in record else []):
            visible.add(view['range_id'])
            for ref in context.sources.get(view['range_id'], []):
                key = (ref['source_index_ref']['sha256'], ref['unit_id'])
                if key in grids:
                    if key in seen:
                        parents[root(number)] = root(seen[key])
                    else:
                        seen[key] = number
    grouped = {}
    for number, record in enumerate(records):
        grouped.setdefault(root(number), []).append(record)
    combined = []
    for group in grouped.values():
        if len(group) == 1:
            combined.extend(group)
            continue
        sources, rules = {}, {}
        for record in group:
            for view in record.get('sources', [record['source']] if 'source' in record else []):
                sources.setdefault(view['range_id'], view)
            for rule in record.get('rules', []):
                rules.setdefault(rule['rule_id'], rule)
        combined.append({'sources': list(sources.values()), 'rules': list(rules.values())})
    for grid in grids.values():
        if set(grid['range_ids']) <= visible:
            expose(grid['source_ref'], [], list(grid['range_ids']))
    return combined, parts
