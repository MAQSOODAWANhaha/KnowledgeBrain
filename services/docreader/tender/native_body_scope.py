"""Final-purpose source census and native candidates; never infer a body column."""
from copy import deepcopy
import hashlib
import json

from docreader.integrations.artifacts import ArtifactIntegrityError
from docreader.tender.authoring_excerpts import response_table_excerpts, source_excerpts_text
from docreader.tender.authoring_templates import load_template_part
from docreader.tender.directory_choices import reference_exclusions
from docreader.tender.directory_compile import _assign_ids, _nest
from docreader.tender.evidence.references import EvidenceRepository, fragment, selector
from docreader.tender.native_body_selection import NativeBodySelectionView
from docreader.tender.native_body_prose import NativeProseSources
from docreader.tender.source_body import load_source_body
from docreader.tender.source_presentation import _render_slice

CONTRACT = 'final_native_body_scope_v1'


def _sha(value):
    return hashlib.sha256(value.encode()).hexdigest()


def _digest(value):
    return _sha(json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':'), allow_nan=False))


def _walk(chapters):
    for chapter in chapters:
        yield chapter
        yield from _walk(chapter.get('children', []))


def _windows(rows):
    result = []
    for row in rows:
        point = {k: row[k] for k in ('document_id', 'page')}
        line = row['line']
        if (result and all(result[-1][k] == v for k, v in point.items())
                and result[-1]['end_line'] + 1 == line):
            result[-1]['end_line'] = line
        else:
            result.append({**point, 'start_line': line, 'end_line': line})
    return result


class NativeBodyScope:
    def __init__(self, artifacts, snapshot, documents, compiled, outline, *, repository=None,
                 owner_for_chapter=None, decoration_applications=None):
        self.artifacts, self.snapshot = artifacts, deepcopy(snapshot)
        self.repository = repository or EvidenceRepository(artifacts)
        frozen = artifacts.read_json(snapshot['frozen_ref'])['documents']
        self.frozen = {d['document_id']: d for d in frozen}
        self.documents = {d['document_id']: d for d in documents}
        if (outline['authoring_input'] != snapshot or len(self.frozen) != len(frozen)
                or len(self.documents) != len(documents) or set(self.documents) != set(self.frozen)):
            raise ArtifactIntegrityError('body scope requires the exact frozen input and reader documents')
        self.cells, self.tables, self._cell_bindings = [], {}, {}
        for source in frozen:
            index = self.repository.source_index(source['source_index_ref'])
            if any(index.document[k] != source[k] for k in ('document_id', 'document_version', 'original_ref')):
                raise ArtifactIntegrityError('body scope native source identity changed')
            for unit in index.raw['units']:
                if not unit.get('grid'):
                    continue
                table = (source['document_id'], unit['unit_id'])
                ids = []
                for cell in unit['grid']['cells']:
                    cid = len(self.cells) + 1
                    self.cells.append({'cell_id': cid, 'document_id': source['document_id'],
                        'table_id': len(self.tables) + 1, **deepcopy(cell)})
                    self._cell_bindings[cid] = (source, index, unit['unit_id'], cell)
                    ids.append(cid)
                self.tables[table] = {'cell_ids': ids, 'page': (unit.get('locator') or {}).get('page_ordinal')}
        self._prose_ids, self._prose_bindings = {}, {}
        self.owners = {c['id']: c for c in compiled['chapters']}
        if owner_for_chapter is None:
            nodes = {}
            _assign_ids(_nest(deepcopy(compiled['chapters']), source_nodes=nodes))
            finals = {c['chapter_id']: c for c in _walk(outline['chapters'])}
            owner_for_chapter = {n['chapter_id']: identity for identity, n in nodes.items()
                if n['chapter_id'] in finals and n['title'] == finals[n['chapter_id']]['title']}
        finals = {c['chapter_id']: c for c in _walk(outline['chapters'])}
        self.owner_for_chapter = {}
        for chapter_id, binding in owner_for_chapter.items():
            identity = binding.get('source_owner_id') if isinstance(binding, dict) else binding
            owner, final = self.owners.get(identity), finals.get(chapter_id)
            if owner is None or final is None:
                continue
            if isinstance(binding, dict):
                valid = (binding.get('source_owner_title') == owner['title']
                         and binding.get('chapter_title') == final['title'])
            else:
                valid = owner.get('display_title', owner['title']) == final['title']
            if valid:
                self.owner_for_chapter[chapter_id] = identity
        origins = compiled.get('_source_renderings', [])
        self.origins = {r['target_id']: r for r in origins}
        if len(self.origins) != len(origins):
            raise ArtifactIntegrityError('duplicate source rendering target identity')
        self.applications = (compiled.get('_reviewed_source_fact_applications', [])
                             if decoration_applications is None else decoration_applications)
        self._prose = NativeProseSources(self.repository, frozen, self.documents, self.applications)
        self.constraints = reference_exclusions(compiled)
        self.records, self.pending, self.bindings, self._units, self._rows = [], [], {}, [], []
        self._row_ids, self._owner_ids, self.exact_bindings, self._body_cache = {}, {}, {}, {}
        targets = [(c, t) for c in _walk(outline['chapters']) for t in c.get('targets', [])]
        targets += [(None, t) for t in outline.get('unplaced_targets', [])]
        if len({t['target_id'] for _, t in targets}) != len(targets):
            raise ArtifactIntegrityError('duplicate final target identity')
        for chapter, target in targets:
            self._target(chapter, target)
        self.orphan_renderings = [deepcopy(r) for r in origins if r['target_id'] not in {t['target_id'] for _, t in targets}]
        for record in self.orphan_renderings:
            retired = any(t.get('target', {}).get('target_id') == record['target_id']
                for app in self.applications if app.get('state') == 'presentation_excluded'
                for t in app.get('retired_targets', []))
            if not retired:
                self._pending({'target_id': record['target_id'], 'chapter_id': record.get('chapter_id')},
                              'source_rendering_without_final_owner')
        self.binding = _digest({'contract': CONTRACT, 'authoring_input': snapshot,
            'outline_sha256': _digest(outline), 'records': self.records, 'rows': self._rows,
            'applications_sha256': _digest(self.applications)})

    def _pending(self, record, reason):
        self.pending.append({k: record.get(k) for k in ('target_id', 'chapter_id')} | {
            'field': 'purpose', 'reason': reason, 'outlet': 'manual_source_review', 'enterprise_confirmed': False})

    def _reference(self, excerpt):
        ref = excerpt['source_ref']['source_index_ref']
        index = self.repository.source_index(ref)
        frozen = self.frozen.get(index.document['document_id'])
        if frozen is None or frozen['source_index_ref'] != ref:
            raise ArtifactIntegrityError('source selection is outside the current frozen input')

    def _target(self, chapter, target):
        record = {'target_id': target['target_id'], 'chapter_id': chapter['chapter_id'] if chapter else None,
                  'field': 'purpose', 'purpose_sha256': _sha(target['purpose']), 'subject': target.get('subject')}
        self.records.append(record)
        owner_id = self.owner_for_chapter.get(record['chapter_id'])
        owner = self.owners.get(owner_id)
        if owner is None:
            record['classification'] = 'owner_pending'
            self._pending(record, 'final_target_without_source_owner')
            return
        record['source_owner_id'] = owner_id
        try:
            if target.get('source_body_ref') or target.get('source_body_contract'):
                load_source_body(self.artifacts, self.snapshot, target, repository=self.repository, cache=self._body_cache)
                record['classification'] = 'source_body_bound'
                return
            carrier, carrier_units, carrier_texts = None, set(), []
            if target.get('template_refs'):
                for part in target['template_refs']:
                    self._reference(part)
                    loaded = load_template_part(self.artifacts, part, repository=self.repository)
                    carrier_texts.append(loaded['text'])
                    if not part.get('selectors') and loaded.get('grid'):
                        index = self.repository.source_index(part['source_ref']['source_index_ref'])
                        carrier_units.add((index.document['document_id'], part['source_ref']['unit_id']))
                carrier = 'template_carried'
            if target.get('response_table'):
                for excerpt in response_table_excerpts(target['response_table']):
                    self._reference(excerpt)
                    carrier_texts.append(source_excerpts_text(self.artifacts, [excerpt], repository=self.repository))
                carrier = carrier or 'response_table_carried'
            if carrier:
                record['carrier_valid'] = True
                record['carrier_kind'] = carrier
                if '\n'.join(carrier_texts) == target['purpose']:
                    record.update(classification=carrier, purpose_role='exact_carried_text')
                    return
            if target.get('source_excerpts'):
                for excerpt in target['source_excerpts']:
                    self._reference(excerpt)
                exact = self._excerpt_binding(target)
                if carrier:
                    record.update(classification=carrier, purpose_role='exact_source_excerpts')
                    return
                for excerpt in target.get('source_context_excerpts', []):
                    self._reference(excerpt)
                    source_excerpts_text(self.artifacts, [excerpt], repository=self.repository)
                record['classification'] = 'source_excerpts_bound'
                self.exact_bindings[target['target_id']] = exact
                return
            origin = self.origins.get(target['target_id'])
            if origin is None or origin['chapter_id'] != record['chapter_id']:
                raise ArtifactIntegrityError('final purpose has no matching source rendering')
            if origin.get('generated'):
                if origin['purpose_sha256'] != record['purpose_sha256']:
                    raise ArtifactIntegrityError('generated purpose changed after compilation')
                record.update(classification=carrier or 'generated', purpose_role='generated')
                return
            rows, slices, exclusions, effective = self._projection(target, origin)
            record.update(source_islands=_windows(rows), rendered_rows=slices, presentation_exclusions=exclusions)
            if carrier:
                visible_rows = [row for row, value in zip(rows, effective) if value.strip()]
                if visible_rows and all((row['document_id'], self._unit_at(row)) in carrier_units for row in visible_rows):
                    record.update(classification=carrier, purpose_role='native_carrier_projection')
                    return
                raise ArtifactIntegrityError('valid original carrier does not prove the additional purpose source')
            islands, unmapped = [], []
            for ordinal, window in enumerate(record['source_islands'], 1):
                visible = [r for r, value in zip(rows, effective) if value.strip()
                    and all(r[k] == window[k] for k in ('document_id', 'page'))
                    and window['start_line'] <= r['line'] <= window['end_line']]
                if not visible:
                    continue  # Full old island remains in source_islands as provenance.
                touched = set()
                for row in visible:
                    uid = self._unit_at(row)
                    if (row['document_id'], uid) in self.tables:
                        touched.add((row['document_id'], uid))
                    elif row['text'][row.get('render_start', 0):row.get('render_end', len(row['text']))].strip():
                        unmapped.append(self._row_ids[(row['document_id'], row['page'], row['line'])])
                cells = sorted(c for table in touched for c in self.tables[table]['cell_ids'] if self.cells[c - 1]['text'].strip())
                islands.append({'island_id': ordinal, 'cell_ids': cells})
            record['layout_pending_rows'] = list(dict.fromkeys(unmapped))
            if not any(i['cell_ids'] for i in islands):
                pages = {(row['document_id'], row['page']) for row, value in zip(rows, effective) if value.strip()}
                prose = self._prose.match(target['purpose'], pages)
                if prose is not None:
                    record.update(source_kind='native_prose', native_prose_source=deepcopy(prose['source_excerpt']),
                                  native_prose_excluded_decorations=deepcopy(prose['excluded_decorations']),
                                  native_prose_generated_syntax=deepcopy(prose['generated_syntax']))
                    if prose['full_range_exact']:
                        record['classification'] = 'native_prose_exact'
                        self.exact_bindings[target['target_id']] = {
                            'parts': [{'source_excerpt': deepcopy(prose['source_excerpt']), 'join': 'paragraphs'}],
                            'context_source_excerpts': []}
                        return
                    source, index, base = prose['source'], prose['index'], prose['base']
                    key = (source['source_index_ref']['sha256'], base['unit_id'], base['range_id'])
                    if key not in self._prose_ids:
                        cid = len(self.cells) + 1
                        self._prose_ids[key] = cid
                        self._prose_bindings[cid] = (source, index, base)
                        self.cells.append({'cell_id': cid, 'source_kind': 'native_prose', 'document_id': source['document_id'],
                            'native_unit_id': base['unit_id'], 'native_range_id': base['range_id'],
                            'page': prose['page'], 'text': base['text']})
                    for island in islands:
                        island['cell_ids'] = [self._prose_ids[key]]
                    record['anchor_spans'] = [[self._prose_ids[key], prose['match_start'] - base['start'],
                                               prose['match_end'] - base['start']]]
                    record['native_prose_layout_rows'] = record['layout_pending_rows']
                    record['layout_pending_rows'], unmapped = [], []
            if not any(i['cell_ids'] for i in islands):
                record['classification'] = 'layout_pending'
                self._pending(record, 'layout_native_ownership_unverified')
                return
            record['classification'] = 'native_resolution'
            uid = len(self._units) + 1
            oid = self._owner_ids.setdefault(owner_id, len(self._owner_ids) + 1)
            self._units.append({'unit_id': uid, 'owner_id': oid, 'subject': target.get('subject'),
                'kind': target['kind'], 'islands': islands, 'rendered_rows': slices,
                'presentation_exclusions': exclusions, 'source_islands': record['source_islands'],
                **({'source_kind': 'native_prose', 'anchor_spans': deepcopy(record['anchor_spans'])}
                   if record.get('source_kind') == 'native_prose' else {})})
            self.bindings[uid] = deepcopy(record)
            if unmapped:
                self._pending(record, 'mixed_native_layout_ownership_unverified')
        except (ArtifactIntegrityError, KeyError, IndexError, TypeError, ValueError) as error:
            record['classification'] = 'unmapped_pending'
            record['source_error'] = str(error)
            self._pending(record, 'source_projection_or_binding_invalid')

    def _excerpt_binding(self, target):
        excerpts = target['source_excerpts']
        context = target.get('source_context_excerpts', [])
        default_exact = source_excerpts_text(self.artifacts, excerpts, repository=self.repository) == target['purpose']
        applications = [app for app in self.applications if app.get('kind') == 'independent_body_response'
            and app.get('state') == 'body_bound' and target['target_id'] in app.get('target_ids', [])]
        join = 'paragraphs'
        if applications:
            if any(app.get('source_excerpts') != excerpts or app.get('source_text_sha256') != _sha(target['purpose'])
                    or app.get('source_join') not in (None, 'paragraphs', 'continuation')
                    or app.get('source_join') is None and not default_exact for app in applications):
                raise ArtifactIntegrityError('current body receipt disagrees with its exact target source')
            recipes = {(app.get('source_join') or 'paragraphs', _digest(app.get('context_source_excerpts', [])))
                       for app in applications}
            if len(recipes) != 1:
                raise ArtifactIntegrityError('explicit body receipts disagree about source joins or context')
            join = applications[0].get('source_join') or 'paragraphs'
            reviewed_context = applications[0].get('context_source_excerpts', [])
            if context and context != reviewed_context:
                raise ArtifactIntegrityError('public source context conflicts with its current body receipt')
            context = reviewed_context
            if source_excerpts_text(self.artifacts, excerpts, source_join=join, repository=self.repository) != target['purpose']:
                raise ArtifactIntegrityError('explicit body receipt cannot reconstruct its exact purpose')
        elif not default_exact:
            raise ArtifactIntegrityError('purpose does not equal its exact source excerpts or explicit body receipt')
        body = {}
        for excerpt in excerpts:
            for selected in excerpt['selectors']:
                key = (excerpt['source_ref']['source_index_ref']['sha256'], excerpt['source_ref']['unit_id'], selected['source_range_id'])
                body.setdefault(key, []).append((selected['start'], selected['end']))
        for excerpt in context:
            self._reference(excerpt)
            source_excerpts_text(self.artifacts, [excerpt], repository=self.repository)
            for selected in excerpt['selectors']:
                key = (excerpt['source_ref']['source_index_ref']['sha256'], excerpt['source_ref']['unit_id'], selected['source_range_id'])
                if any(low < selected['end'] and selected['start'] < high for low, high in body.get(key, [])):
                    raise ArtifactIntegrityError('exact body and source context must stay disjoint')
        return {'parts': [{'source_excerpt': deepcopy(excerpt), 'join': 'paragraphs' if i == 0 else join}
                          for i, excerpt in enumerate(excerpts)], 'context_source_excerpts': deepcopy(context)}

    def _projection(self, target, origin):
        rows = origin['rows']
        if origin['rows_sha256'] != _digest(rows):
            raise ArtifactIntegrityError('source rendering row hash changed')
        slices, originals, rendered, exclusions, effective = [], [], [], [], []
        for row in rows:
            if any(type(row[k]) is not int or row[k] < 0 for k in ('page', 'line')):
                raise ArtifactIntegrityError('source rendering coordinates must be nonnegative integers')
            text = self.documents[row['document_id']]['pages'][row['page']].splitlines()[row['line']]
            if text != row['text']:
                raise ArtifactIntegrityError('source rendering line bytes changed')
            start, end, visible = _render_slice(row)
            point = (row['document_id'], row['page'], row['line'])
            if point not in self._row_ids:
                self._row_ids[point] = len(self._rows) + 1
                self._rows.append([self._row_ids[point], *point, text])
            slices.append([self._row_ids[point], start, end, visible])
            if not visible:
                effective.append('')
                continue
            value = text[start:end]
            originals.append(value)
            fragments = []
            for app in self.applications:
                if (app.get('kind') != 'source_decoration' or app.get('state') != 'presentation_excluded'
                        or target['target_id'] not in app.get('affected_target_ids', [])
                        or app.get('original_ref') != self.frozen[row['document_id']]['original_ref']):
                    continue
                for original in app.get('original_fragments', []):
                    if (original['page'], original['line']) != point[1:]:
                        continue
                    low, high = original['start'], original['end']
                    if (app['original_ref'] != self.frozen[row['document_id']]['original_ref']
                            or original['line_text'] != text or not 0 <= low < high <= len(text)
                            or _sha(text[low:high]) != original['text_sha256']):
                        raise ArtifactIntegrityError('decoration application does not match its frozen source')
                    if max(start, low) < min(end, high):
                        fragments.append((low, high))
                        exclusions.append({'row_id': self._row_ids[point], 'start': low, 'end': high, 'fact_id': app['fact_id']})
            occupied = -1
            for low, high in sorted(fragments):
                if low < occupied:
                    raise ArtifactIntegrityError('decoration applications overlap')
                occupied = high
            for low, high in sorted(fragments, reverse=True):
                value = value[:max(start, low) - start] + value[min(end, high) - start:]
            rendered.append(value)
            effective.append(value)
        if (_sha('\n'.join(originals).strip()) != origin['purpose_sha256']
                or '\n'.join(rendered).strip() != target['purpose']):
            raise ArtifactIntegrityError('source rendering cannot reconstruct the exact current purpose')
        return rows, slices, exclusions, effective

    def _unit_at(self, row):
        units = self.documents[row['document_id']].get('line_units') or []
        return (units[row['page']][row['line']]
                if row['page'] < len(units) and row['line'] < len(units[row['page']]) else None)

    def selection_view(self):
        return NativeBodySelectionView(self.cells, self._units).subset([u['unit_id'] for u in self._units])

    def request_payload(self, view):
        identities = {u['unit_id'] for u in view.units}
        if not identities <= set(self.bindings):
            raise ArtifactIntegrityError('selection view has unknown source scope units')
        expected = self.selection_view().subset([u['unit_id'] for u in view.units])
        if expected.payload() != view.payload():
            raise ArtifactIntegrityError('selection view changed its immutable source scope')
        rows = {r[0] for u in view.units for r in u['rendered_rows']}
        owners = {u['owner_id'] for u in view.units}
        return {'operation': CONTRACT, 'scope_binding': self.binding,
            **view.payload(), 'source_row_fields': ['id', 'document_id', 'page', 'line', 'text'],
            'source_rows': [r for r in self._rows if r[0] in rows],
            'owners': [{'owner_id': short, **{k: deepcopy(owner[k]) for k in
                ('title', 'heading', 'entry', 'subjects', 'body_windows', 'clause_windows', 'condition_windows') if k in owner}}
                for identity, short in self._owner_ids.items() if short in owners for owner in [self.owners[identity]]],
            'fixed_source_exclusions': deepcopy(self.constraints), 'fact_changes_allowed': False}

    def cell_excerpt(self, cell_id, start, end):
        if (type(cell_id) is not int or cell_id not in {*self._cell_bindings, *self._prose_bindings} or type(start) is not int
                or type(end) is not int or not 0 <= start < end <= len(self.cells[cell_id - 1]['text'])):
            raise ArtifactIntegrityError('body cell interval is outside its frozen source')
        if cell_id in self._prose_bindings:
            source, index, base = self._prose_bindings[cell_id]
            return {'source_ref': {'source_index_ref': deepcopy(source['source_index_ref']), 'unit_id': base['unit_id']},
                    'selectors': [selector(fragment(base, base['start'] + start, base['start'] + end))]}
        source, index, unit_id, cell = self._cell_bindings[cell_id]
        candidates = [(base, interval) for base in index.ranges if base['unit_id'] == unit_id
            for interval in base.get('cell_intervals', []) if interval['row'] == cell['row'] and interval['column'] == cell['column']
            and index.materialize(selector(fragment(base, interval['start'], interval['end'])))['text'] == cell['text']]
        if len(candidates) != 1:
            raise ArtifactIntegrityError('body cell has no unique exact native range binding')
        base, interval = candidates[0]
        selected = fragment(base, interval['start'] + start, interval['start'] + end)
        return {'source_ref': {'source_index_ref': deepcopy(source['source_index_ref']), 'unit_id': unit_id},
                'selectors': [selector(selected)]}
