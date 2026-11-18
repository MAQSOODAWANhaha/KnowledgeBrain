"""Bounded native response-form selection; source prose is materialized by the host."""

from copy import deepcopy
from functools import lru_cache
import hashlib
import json
import re
from typing import Annotated, Literal

from pydantic import Field, StrictInt, ValidationError, create_model

from docreader.llm.request import ModelBudgetError, ModelContractError
from docreader.tender.authoring_excerpts import response_row_id, source_excerpts_text
from docreader.tender.authoring_templates import load_template_part, template_part_key
from docreader.tender.authoring_view import _grid_view
from docreader.tender.directory_compile import (
    _Source, _source_tables, _table_id, _window, _window_text,
)
from docreader.tender.evidence.references import EvidenceRepository, fragment, selector
from docreader.tender.native_response_table import prepare_native_response_table
from docreader.tender.schema import Contract


RESPONSE_FORM_CONTRACT = 'source_bound_response_forms_v1'
RESPONSE_FORM_DECISION_CONTRACT = 'source_bound_response_forms_v2'
RESPONSE_FORM_ROLE_CONTRACT = 'source_bound_response_forms_v3'
RESPONSE_FORM_ROLE_OPERATION = 'select_native_response_form_v3'
RESPONSE_FORM_ROLE_RECORD = 'outline_directory_response_form_row_roles'
RESPONSE_FORM_CONTRACTS = (RESPONSE_FORM_CONTRACT, RESPONSE_FORM_DECISION_CONTRACT, RESPONSE_FORM_ROLE_CONTRACT)
_KINDS = Literal['criterion', 'context', 'reference', 'procedure', 'navigation', 'unresolved']
_ROW_ROLES = Literal['criterion', 'reference', 'procedure']
_JOIN = Literal['paragraphs', 'continuation']


def _json(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':'))


def _reject(detail, path=()):
    raise ModelContractError([{'path': list(path), 'code': 'directory_response_form_invalid', 'detail': detail}])


def _chapter_parts(compiled):
    seen = set()
    for chapter in compiled['chapters']:
        if chapter.get('unplaced') or chapter.get('response_kind', 'form') != 'form':
            continue
        parts = [*chapter.get('template_parts', []), *(segment['part']
            for key in ('template_segments', 'material_segments') for segment in chapter.get(key, []) if segment.get('part'))]
        for part in parts:
            key = chapter['id'], template_part_key(part)
            if not part.get('selectors') and key not in seen:
                seen.add(key)
                yield chapter, part


def response_form_catalog(compiled, documents):
    tables, result = _source_tables(documents), {}
    for chapter, part in _chapter_parts(compiled):
        identity = _table_id(part)
        known = tables.get(identity)
        if not known or not known.get('grid'):
            continue
        entry = result.setdefault(identity, {'table_id': identity, 'document_id': known['document_id'],
            'page': known['page'], 'chapter_ids': [], 'row_count': known['grid']['row_count'],
            'column_count': known['grid']['column_count']})
        if chapter['id'] not in entry['chapter_ids']:
            entry['chapter_ids'].append(chapter['id'])
    return list(result.values())


def validate_response_form_selections(compiled, documents, selections, *, supplied=None):
    source = _Source(documents)
    known = {entry['table_id'] for entry in response_form_catalog(compiled, documents)}
    allowed = known if supplied is None else {entry['table_id'] for entry in supplied}
    seen = set()
    for number, selected in enumerate(selections):
        identity = selected['table_id']
        if identity not in known & allowed or identity in seen:
            _reject('Select each supplied, currently adopted native response form at most once.', ('response_forms', number))
        seen.add(identity)
        for key in ('mandate_windows', 'source_windows'):
            if not selected[key]:
                _reject('A response form needs its mandate and complete source scope.', ('response_forms', number, key))
            for window in selected[key]:
                try:
                    value = _window(window, source, key)
                    if not _window_text(source, value).strip():
                        raise ValueError('empty source window')
                except ValueError:
                    _reject('Select a nonempty window inside the registered source.', ('response_forms', number, key))


@lru_cache(maxsize=128)
def _response_model(count, row_count, column_count):
    ref = Annotated[StrictInt, Field(ge=1, le=count)]
    refs = Annotated[list[ref], Field(min_length=1, max_length=count)]
    context = create_model('ResponseFormContext', __base__=Contract, refs=(refs, ...))
    row = create_model('ResponseFormRow', __base__=Contract, refs=(refs, ...),
        context_index=(Annotated[StrictInt, Field(ge=0, le=max(0, count - 1))] | None, None), source_join=(_JOIN, 'paragraphs'))
    guidance = create_model('ResponseFormGuidance', __base__=Contract,
        kind=(Literal['reference', 'procedure'], ...), refs=(refs, ...), source_join=(_JOIN, 'paragraphs'))
    disposition = create_model('ResponseFormDisposition', __base__=Contract, refs=(refs, ...), kind=(_KINDS, ...))
    layout = create_model('ResponseFormLayout', __base__=Contract,
        **{key: (Annotated[StrictInt, Field(ge=0, le=limit)], ...) for key, limit in [
            ('repeat_start_row', row_count - 1), ('repeat_end_row', row_count - 1),
            *[(key, column_count - 1) for key in ('sequence_column', 'source_column', 'response_column', 'proof_column')]]})
    return create_model('ResponseFormPlan', __base__=Contract, layout=(layout | None, None),
        contexts=(Annotated[list[context], Field(max_length=count)], Field(default_factory=list)),
        table_context_refs=(Annotated[list[ref], Field(max_length=count)], Field(default_factory=list)),
        rows=(Annotated[list[row], Field(max_length=count)], Field(default_factory=list)),
        source_guidance=(Annotated[list[guidance], Field(max_length=count)], Field(default_factory=list)),
        dispositions=(Annotated[list[disposition], Field(min_length=1, max_length=count)], ...))


@lru_cache(maxsize=128)
def _decision_model(count, row_count, column_count, mandate_count):
    original = _response_model(count, row_count, column_count)
    indices = Annotated[list[Annotated[StrictInt, Field(ge=0, lt=mandate_count)]], Field(max_length=mandate_count)]
    return create_model('ResponseFormDecisionPlan', __base__=original,
        decision=(Literal['expand', 'keep_original', 'unresolved'], ...), mandate_indices=(indices, ...),
        dispositions=(Annotated[original.model_fields['dispositions'].annotation,
                                Field(max_length=count)], Field(default_factory=list)))


@lru_cache(maxsize=128)
def _role_model(count, row_count, column_count, mandate_count):
    original = _decision_model(count, row_count, column_count, mandate_count)
    row = create_model('ResponseFormRoleRow', __base__=original.model_fields['rows'].annotation.__args__[0],
                       source_role=(_ROW_ROLES, ...))
    return create_model('ResponseFormRoleDecisionPlan', __base__=original,
                        rows=(Annotated[list[row], Field(max_length=count)], Field(default_factory=list)))


def response_form_model(payload):
    grid = payload['form']['grid']
    if payload.get('operation') == RESPONSE_FORM_ROLE_OPERATION:
        return _role_model(payload['atom_count'], grid['row_count'], grid['column_count'], len(payload['mandate']))
    if payload.get('operation') == 'select_native_response_form_v2':
        return _decision_model(payload['atom_count'], grid['row_count'], grid['column_count'], len(payload['mandate']))
    return _response_model(payload['atom_count'], grid['row_count'], grid['column_count'])


def _page(unit):
    for locator in (unit.get('physical_locator'), unit.get('locator')):
        if isinstance(locator, dict) and type(locator.get('page_ordinal')) is int:
            return locator['page_ordinal']
    match = re.search(r'(?:^|:)page:(\d+)(?:$|:)', unit['unit_id'])
    return int(match[1]) if match else None


def _lines(text):
    result, offset, prefix = [], 0, ''
    for line in text.splitlines(keepends=True):
        if line.strip():
            result.append([offset - len(prefix), prefix + line])
            prefix = ''
        elif result:
            result[-1][1] += line
        else:
            prefix += line
        offset += len(line)
    return result


class ResponseFormView:
    def __init__(self, artifacts, compiled, documents, selection, snapshot, *, contract=RESPONSE_FORM_CONTRACT):
        if contract not in RESPONSE_FORM_CONTRACTS:
            raise ValueError('unsupported native response form contract')
        validate_response_form_selections(compiled, documents, [selection])
        self.artifacts, self.repository = artifacts, EvidenceRepository(artifacts)
        self.selection, self.atoms = deepcopy(selection), {}
        self.owners, parts = [], []
        for chapter, part in _chapter_parts(compiled):
            if _table_id(part) == selection['table_id']:
                self.owners.append(chapter['id'])
                parts.append(part)
        if not parts or any(part != parts[0] for part in parts[1:]):
            _reject('The selected native form has conflicting adopted instances.')
        self.part = deepcopy(parts[0])
        loaded = load_template_part(artifacts, self.part, repository=self.repository)
        form_grid = loaded['unit'].get('grid')
        if not form_grid:
            _reject('The selected form has no complete native grid.')
        frozen = artifacts.read_json(snapshot['frozen_ref'])
        frozen_documents = {record['document_id']: record for record in frozen['documents']}
        requested = {}
        for window in selection['source_windows']:
            requested.setdefault(window['document_id'], set()).add(window['page'])
        blocks = []
        ordered = {record['document_id']: requested[record['document_id']]
                   for record in frozen['documents'] if record['document_id'] in requested}
        if ordered.keys() != requested.keys():
            _reject('Every source-scope document must exist in the fixed execution source inventory.')
        requested = ordered
        for document_number, (document_id, pages) in enumerate(requested.items(), 1):
            record = frozen_documents[document_id]
            index_ref = record['source_index_ref']
            index = self.repository.source_index(index_ref)
            if index.document['document_id'] != document_id:
                _reject('The source index belongs to another registered document.')
            units = {unit['unit_id']: unit for unit in index.raw['units']}
            for base in index.ranges:
                unit = units[base['unit_id']]
                page = _page(unit)
                if page is None:
                    if unit.get('locator', {}).get('locator_kind') == 'attachment':
                        continue
                    _reject('The complete native source scope has an unlocated source unit; keep the original form for review.')
                if page not in pages:
                    continue
                reference = {'source_index_ref': index_ref, 'unit_id': unit['unit_id']}
                common = {'document': document_number, 'page': page, 'unit': unit['ordinal']}
                if base.get('text_basis') == 'grid_row':
                    cells = []
                    for cell in base['cell_intervals']:
                        lines = self._atoms(base, reference, cell['text'], cell['start'])
                        cells.append({'column': cell['column'], 'row_span': cell['row_span'],
                                      'col_span': cell['col_span'], 'lines': lines})
                    blocks.append({**common, 'kind': 'native_row', 'row': base['row_start'],
                                   'column_count': base['column_count'], 'cells': cells})
                else:
                    blocks.append({**common, 'kind': base['kind'], 'lines': self._atoms(base, reference, base['text'], base['start'])})
        if not self.atoms:
            _reject('The selected native source scope contains no readable clause candidates.')
        source = _Source(documents)
        mandate = [{'document': window['document_id'], 'page': window['page'],
                    'text': _window_text(source, _window(window, source, 'mandate'))}
                   for window in selection['mandate_windows']]
        if contract in (RESPONSE_FORM_DECISION_CONTRACT, RESPONSE_FORM_ROLE_CONTRACT):
            for entry, window in zip(mandate, selection['mandate_windows']):
                entry.update(start_line=window['start_line'], end_line=window['end_line'])
        binding = {'contract': contract, 'form': self.part, 'selection': selection,
                   'atoms': [(ref, atom['source_ref'], atom['start'], atom['end']) for ref, atom in self.atoms.items()]}
        self.payload = {'operation': (RESPONSE_FORM_ROLE_OPERATION if contract == RESPONSE_FORM_ROLE_CONTRACT
                                     else 'select_native_response_form_v2' if contract == RESPONSE_FORM_DECISION_CONTRACT
                                     else 'select_native_response_form_v1'), 'response_form_contract': contract,
            'source_binding': hashlib.sha256(_json(binding).encode()).hexdigest(),
            'form': {'grid': _grid_view(form_grid), 'header_row_count': self.part.get('header_row_count')},
            'mandate': mandate, 'source_scope': blocks, 'atom_count': len(self.atoms)}
        self.payload['source_scope_closure'] = 'complete_native_pages_selected_by_source_windows_v1'
        self.payload['source_pages'] = [{'document': number, 'pages': sorted(pages)}
                                       for number, pages in enumerate(requested.values(), 1)]

    def _atoms(self, base, source_ref, text, start):
        result = []
        for offset, line in _lines(text):
            identity = len(self.atoms) + 1
            self.atoms[identity] = {'base': base, 'source_ref': source_ref,
                                    'start': start + offset, 'end': start + offset + len(line)}
            result.append([identity, line])
        return result

    def excerpts(self, refs):
        if refs != sorted(set(refs)):
            _reject('Select source fragments once in their original document/cell order.')
        result = []
        previous = None
        for identity in refs:
            atom = self.atoms[identity]
            key = atom['source_ref'], atom['base']['source_range_id']
            if result and previous == key and result[-1]['selectors'][-1]['end'] == atom['start']:
                first = result[-1]['selectors'][-1]['start']
                result[-1]['selectors'][-1] = selector(fragment(atom['base'], first, atom['end']))
            else:
                result.append({'source_ref': deepcopy(atom['source_ref']),
                               'selectors': [selector(fragment(atom['base'], atom['start'], atom['end']))]})
            previous = key
        return result

    def accept(self, raw):
        try:
            value = response_form_model(self.payload).model_validate(raw).model_dump()
        except ValidationError as error:
            raise ModelContractError([{'path': list(entry['loc']), 'code': entry['type'], 'detail': entry['msg']}
                                      for entry in error.errors(include_input=False)]) from None
        decision = value.pop('decision', None)
        indices = value.pop('mandate_indices', [])
        if decision is not None:
            if len(indices) != len(set(indices)) or (decision != 'unresolved' and not indices):
                _reject('Choose distinct supplied mandate indices for the response-form decision.', ('mandate_indices',))
            decision_record = {'decision': decision, 'mandate_sources': [deepcopy(self.payload['mandate'][i]) for i in indices],
                'mandate_scope': deepcopy(self.payload['mandate']), 'mandate_indices': list(indices),
                'table_source_ref': deepcopy(self.part['source_ref'])}
            if self.payload.get('operation') == RESPONSE_FORM_ROLE_OPERATION:
                decision_record['response_form_contract'] = RESPONSE_FORM_ROLE_CONTRACT
            if decision != 'expand':
                if any(value[key] for key in ('layout', 'contexts', 'table_context_refs', 'rows', 'source_guidance', 'dispositions')):
                    _reject('A preserved or unresolved form cannot also contain expansion selections.')
                return {'table_id': self.selection['table_id'], 'chapter_ids': list(dict.fromkeys(self.owners)),
                    'selection': self.selection, 'source_binding': self.payload['source_binding'],
                    'complete': decision == 'keep_original', **decision_record}
            if not value['dispositions']:
                _reject('An expansion must give every source atom a disposition.', ('dispositions',))
        dispositions = {}
        for entry in value['dispositions']:
            for identity in entry['refs']:
                if identity in dispositions:
                    _reject('Each source atom needs exactly one disposition.')
                dispositions[identity] = entry['kind']
        if set(dispositions) != self.atoms.keys():
            _reject('Disposition coverage must include every supplied source atom.')
        owners = {}

        def own(refs, kind):
            for identity in refs:
                if dispositions[identity] != kind or identity in owners:
                    _reject('Body/context/guidance ownership must match dispositions without repeated source atoms.')
                owners[identity] = kind

        for entry in value['contexts']:
            own(entry['refs'], 'context')
        own(value['table_context_refs'], 'context')
        rows, row_roles, used_contexts = [], [], set()
        for entry in sorted(value['rows'], key=lambda entry: min(entry['refs'])):
            role = entry.get('source_role', 'criterion')
            own(entry['refs'], role)
            row_roles.append(role)
            context = entry['context_index']
            if context is not None and context >= len(value['contexts']):
                _reject('Choose an existing shared context group.')
            if context is not None:
                used_contexts.add(context)
            rows.append({'source_excerpts': self.excerpts(entry['refs']), 'rule_refs': [],
                'context_source_excerpts': self.excerpts(value['contexts'][context]['refs']) if context is not None else [],
                'source_join': entry['source_join']})
        if used_contexts != set(range(len(value['contexts']))):
            _reject('Do not declare unused shared row context.')
        guidance = []
        for entry in value['source_guidance']:
            own(entry['refs'], entry['kind'])
            guidance.append({'kind': entry['kind'], 'source_excerpts': self.excerpts(entry['refs']),
                             'source_join': entry['source_join']})
        for entry in guidance:
            try:
                source_excerpts_text(self.artifacts, entry['source_excerpts'], source_join=entry['source_join'],
                                     repository=self.repository)
            except (ValueError, KeyError, TypeError):
                _reject('Source guidance must use exact, readable fragments and forward continuation in one frozen document.')
        required = {identity for identity, kind in dispositions.items() if kind not in {'navigation', 'unresolved'}}
        if owners.keys() != required:
            _reject('Every substantive disposition must be bound to a row, context or source guidance.')
        layout = ({**value['layout'], 'source_ref': deepcopy(self.part['source_ref'])} if value['layout'] else None)
        complete = bool(layout and rows and 'unresolved' not in dispositions.values())
        plan = {'table_id': self.selection['table_id'], 'chapter_ids': list(dict.fromkeys(self.owners)),
            'selection': self.selection, 'source_binding': self.payload['source_binding'], 'complete': complete,
            'response_table': {'native_layout': layout, 'rows': rows,
                               'context_source_excerpts': self.excerpts(value['table_context_refs']), 'source_guidance': guidance},
            'dispositions': [{'kind': entry['kind'], 'source_excerpts': self.excerpts(entry['refs'])}
                             for entry in value['dispositions']]}
        if decision is not None:
            plan.update(decision_record)
        if self.payload.get('operation') == RESPONSE_FORM_ROLE_OPERATION:
            plan['row_roles'] = row_roles
        if complete:
            temporary = {'target_id': 'source-form-validation', 'kind': 'form', 'manual_fill': True,
                         'template_refs': [self.part], 'response_table': deepcopy(plan['response_table'])}
            _bind_rows(temporary)
            try:
                prepare_native_response_table(self.artifacts, temporary, [self.part], repository=self.repository)
            except (ValueError, KeyError, TypeError) as error:
                _reject('The selected source/layout cannot materialize one complete original form: ' + str(error)[:300])
        return plan


def _bind_rows(target):
    table = target['response_table']
    for row in table['rows']:
        row['row_id'] = response_row_id(target['target_id'], row['source_excerpts'], row['rule_refs'],
            context_source_excerpts=row.get('context_source_excerpts', []),
            source_join=row.get('source_join', 'paragraphs'), native_layout=table['native_layout'])


def response_form_role_record(issue, target=None):
    """Read the dedicated v3 audit record, optionally checking its actual row binding."""
    if issue.get('code') != RESPONSE_FORM_ROLE_RECORD:
        return None
    try:
        record = json.loads(issue['detail'])
        if (record['response_form_contract'] != RESPONSE_FORM_ROLE_CONTRACT
                or record['target_id'] != issue.get('target_id')):
            return None
        roles = record['row_roles']
        if any(entry['row_ordinal'] != ordinal or entry['source_role'] not in ('criterion', 'reference', 'procedure')
               for ordinal, entry in enumerate(roles, 1)):
            return None
        if target is not None:
            rows = (target.get('response_table') or {}).get('rows', [])
            if (target['target_id'] != record['target_id'] or len(rows) != len(roles)
                    or any(row['row_id'] != role['row_id'] for row, role in zip(rows, roles))
                    or not any(part['source_ref'] == record['table_source_ref'] for part in target.get('template_refs', []))):
                return None
        return record
    except (ValueError, KeyError, TypeError):
        return None


def response_form_role_guide(issue, target=None):
    """Keep internal audit fields out of the human submission guide."""
    if target is None:
        return '原式行责任记录与当前响应位置不一致，请对照原表重新核定；企业事实未确认。'
    record = response_form_role_record(issue, target)
    if record is None:
        return '原式行责任记录与当前响应位置不一致，请对照原表重新核定；企业事实未确认。'
    decisions = {'expand': '按来源展开原式', 'keep_original': '保持原式', 'unresolved': '职责待人工核定'}
    labels = {'criterion': '投标人逐项应答条款', 'reference': '参考规则', 'procedure': '评委或他方程序'}
    lines = [decisions[record['decision']] + '。企业事实未确认，不代表已响应、已履行或取得得分。']
    lines.extend(f"第{entry['row_ordinal']}行：{labels[entry['source_role']]}；企业应答和证明位置留空。"
                 for entry in record['row_roles'])
    lines.append('全部已给职责依据：')
    lines.extend(f"第{entry['page'] + 1}页第{entry['start_line'] + 1}–{entry['end_line'] + 1}行：{entry['text']}"
                 for entry in record['mandate_scope'])
    return '\n'.join(lines)


def pending_response_form(artifacts, compiled, documents, selection, snapshot):
    validate_response_form_selections(compiled, documents, [selection])
    matches = [(chapter, part) for chapter, part in _chapter_parts(compiled) if _table_id(part) == selection['table_id']]
    if not matches:
        _reject('The pending form must retain its adopted source instance.')
    frozen = artifacts.read_json(snapshot['frozen_ref'])
    records = {record['document_id']: record for record in frozen['documents']}
    windows = {key: [{**window, 'source_index_ref': records[window['document_id']]['source_index_ref']}
                     for window in selection[key]] for key in ('mandate_windows', 'source_windows')}
    body = {'table_source_ref': deepcopy(matches[0][1]['source_ref']), **windows}
    return {'chapter_ids': list(dict.fromkeys(chapter['id'] for chapter, _ in matches)),
            'pending': {**body, 'source_binding': hashlib.sha256(_json(body).encode()).hexdigest(),
                        'reason': 'source_unavailable'}}


def bind_response_form_targets(compiled, node_map):
    for pending in compiled.get('_response_form_pending', []):
        for chapter_id in pending['chapter_ids']:
            for target in (node_map.get(chapter_id) or {}).get('targets', []):
                if target.get('kind') == 'form' and any(part['source_ref'] == pending['pending']['table_source_ref']
                                                       for part in target.get('template_refs', [])):
                    target['response_form_pending'] = deepcopy(pending['pending'])
    for plan in compiled.get('_response_form_plans', []):
        if not plan['complete']:
            continue
        matched = False
        for chapter_id in plan['chapter_ids']:
            node = node_map.get(chapter_id)
            for target in (node or {}).get('targets', []):
                source_ref = (plan['table_source_ref'] if plan.get('decision') == 'keep_original'
                              else plan['response_table']['native_layout']['source_ref'])
                parts = [part for part in target.get('template_refs', [])
                         if part['source_ref'] == source_ref]
                if len(parts) != 1 or not target.get('manual_fill') or target.get('kind') != 'form':
                    continue
                if plan.get('decision') == 'keep_original':
                    if target.get('response_table'):
                        continue
                else:
                    target['response_table'] = deepcopy(plan['response_table'])
                    _bind_rows(target)
                if plan.get('response_form_contract') == RESPONSE_FORM_ROLE_CONTRACT:
                    record = {key: deepcopy(plan[key]) for key in ('response_form_contract', 'decision',
                        'source_binding', 'mandate_scope', 'mandate_indices', 'table_source_ref')}
                    record.update(target_id=target['target_id'], row_roles=[
                        {'row_id': row['row_id'], 'row_ordinal': ordinal, 'source_role': role}
                        for ordinal, (row, role) in enumerate(zip((target.get('response_table') or {}).get('rows', []),
                                                                 plan.get('row_roles', [])), 1)])
                    issue = {'code': RESPONSE_FORM_ROLE_RECORD, 'target_id': target['target_id'], 'detail': _json(record)}
                    if issue not in compiled['issues']:
                        compiled['issues'].append(issue)
                elif plan.get('decision'):
                    issue = {'code': 'outline_directory_response_form_decision', 'target_id': target['target_id'], 'detail':
                        '原式来源裁定：' + plan['decision'] + '；来源绑定：' + plan['source_binding'] +
                        '\n所选依据索引（零基）：' + ','.join(map(str, plan['mandate_indices'])) +
                        '\n企业事实未确认，不代表已响应或无偏离。全部已给职责依据：\n' +
                        '\n'.join(f"[{index}] {entry['document']} 第{entry['page'] + 1}页 "
                                  f"第{entry['start_line'] + 1}–{entry['end_line'] + 1}行：{entry['text']}"
                                  for index, entry in enumerate(plan['mandate_scope']))}
                    if issue not in compiled['issues']:
                        compiled['issues'].append(issue)
                target.pop('response_form_pending', None)
                matched = True
        if not matched:
            compiled['issues'].append({'code': 'outline_directory_response_form_unbound',
                'detail': '逐条响应原式未能绑定最终编排中的同源实例，原表保留，须核定来源和响应位置。'})


def response_form_output_bound(payload, budget):
    """Measure a conservative linear accepted-graph shape, never source prose."""
    count = payload['atom_count']
    ref = count
    grid = payload['form']['grid']
    layout = {key: limit for key, limit in [
        ('repeat_start_row', grid['row_count'] - 1), ('repeat_end_row', grid['row_count'] - 1),
        *[(key, grid['column_count'] - 1) for key in ('sequence_column', 'source_column', 'response_column', 'proof_column')]]}
    base = {'layout': layout, 'contexts': [], 'table_context_refs': [], 'rows': [], 'source_guidance': [],
            'dispositions': [{'refs': [ref], 'kind': 'unresolved'} for _ in range(count)]}
    variants = []
    for key, item in [('contexts', {'refs': [ref]}),
                      ('rows', {'refs': [ref], 'context_index': count - 1, 'source_join': 'continuation'}),
                      ('source_guidance', {'kind': 'procedure', 'refs': [ref], 'source_join': 'continuation'})]:
        if key == 'rows' and payload.get('operation') == RESPONSE_FORM_ROLE_OPERATION:
            item = {**item, 'source_role': 'criterion'}
        variants.append({**base, key: [item for _ in range(count)]})
    from docreader.llm.budget import _o200k_tokens
    tokenized = budget.tokenizer is not None and budget.tokenized_authoring
    counter = _o200k_tokens if tokenized else lambda text: len(text.encode())
    extra = 0
    if payload.get('operation') in ('select_native_response_form_v2', RESPONSE_FORM_ROLE_OPERATION):
        extra = len(_json({'decision': 'keep_original', 'mandate_indices': list(range(len(payload['mandate'])))}).encode())
    return max(counter(_json(value)) for value in variants) + 4 * count + 256 + extra


def require_response_form_budget(budget, request, payload):
    measured = budget.require_request(request, 'directory_response_form')
    bound = response_form_output_bound(payload, budget)
    if bound > measured['output_reserve']:
        raise ModelBudgetError('complete response-form scope exceeds the reserved worst accepted output shape')
    return {**measured, 'worst_accepted_output_tokens': bound}
