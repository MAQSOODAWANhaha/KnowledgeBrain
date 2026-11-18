"""Close physical native-carrier differences after the complete M source choice."""

from copy import deepcopy
from typing import Literal

from pydantic import Field

from docreader.llm.request import ModelContractError
from docreader.tender.directory_choices import _compatible, _pending_cases, _source_record
from docreader.tender.directory_compile import _Source, _attach_template_parts, _source_tables, _table_id, _window, _window_text
from docreader.tender.directory_source_groups import MaterialSourceGroupPlan
from docreader.tender.schema import Contract, Identity


SOURCE_CARRIER_CONTRACT = 'source_carriers_closed_v1'
SOURCE_CARRIER_OPERATION = 'select_source_closed_materials_v3'


class CarrierDisposition(Contract):
    table_id: Identity
    action: Literal['reference', 'exclude', 'unresolved']
    chapter_id: Identity | None = None
    case_id: Identity | None = None


class MaterialClosurePlan(MaterialSourceGroupPlan):
    carrier_dispositions: list[CarrierDisposition] = Field(default_factory=list)


def native_parts(chapter):
    parts = list(chapter.get('template_parts') or [])
    for field in ('template_segments', 'material_segments'):
        parts.extend(segment['part'] for segment in chapter.get(field) or [] if segment.get('part'))
    return {_table_id(part): part for part in parts}


def candidate_carriers(compiled, documents):
    """Deduplicate physical tables, retaining every original candidate and source record."""
    source, items = _Source(documents), {}
    for case in _pending_cases(compiled, source).values():
        for option in ('current', 'alternative'):
            candidate = deepcopy(case[option])
            if candidate is None:
                continue
            _attach_template_parts([candidate], source)
            for identity, part in native_parts(candidate).items():
                item = items.setdefault(identity, {'table_id': identity,
                    'source_ref': deepcopy(part['source_ref']), 'origins': []})
                item['origins'].append({'case_id': case['case_id'], 'option': option,
                    'signature': {key: deepcopy(candidate.get(key)) for key in ('response_kind', 'subjects', 'entry')},
                    'source_record': _source_record(candidate, source, 'candidate')})
    return list(items.values())


def source_carrier_catalog(carriers, *, required_native_ids=()):
    """The full source and native parts stay in the host; M selects only finite identities."""
    return [{'table_id': item['table_id'], **({'reviewed_required': True} if item['table_id'] in required_native_ids else {}), 'origins': [
        {key: deepcopy(origin[key]) for key in ('case_id', 'option', 'signature')}
        for origin in item['origins']]} for item in carriers]


def _reject(detail, path=(), code='directory_material_carrier_invalid'):
    raise ModelContractError([{'path': ['carrier_dispositions', *path], 'code': code, 'detail': detail}])


def _location(table, source, identity):
    if table is None or table['page'] is None:
        return f'已核原生载体{identity}（原件定位保存在原核定证据）'
    document = source.by_id[table['document_id']]
    tables = [grid['unit_id'] for grid in document['grids'] if grid.get('page') == table['page']]
    number = f'第{tables.index(table["unit_id"]) + 1}张' if table['unit_id'] in tables else ''
    return f'原件{document["index"] + 1}物理第{table["page"] + 1}页{number}原表'


def apply_carrier_closure(compiled, documents, dispositions, carriers, *, required_native_ids=()):
    """Validate the entire difference before recording results or named pending work."""
    source = _Source(documents)
    source_tables = _source_tables(documents)
    items = {item['table_id']: item for item in carriers}
    explicit = {}
    for index, raw in enumerate(dispositions):
        value = CarrierDisposition.model_validate(raw).model_dump()
        identity = value['table_id']
        if identity not in items or identity in explicit:
            _reject('choose each supplied candidate table at most once', (index, 'table_id'))
        if value['action'] != 'reference' and value['chapter_id'] is not None:
            _reject('only reference selects one canonical chapter', (index, 'chapter_id'))
        if value['case_id'] is not None and value['case_id'] not in {o['case_id'] for o in items[identity]['origins']}:
            _reject('case_id must be an origin of this table in this M request', (index, 'case_id'))
        explicit[identity] = (index, value)
    chapters = {chapter['id']: chapter for chapter in compiled['chapters'] if not chapter.get('unplaced')}
    parts = {identity: native_parts(chapter) for identity, chapter in chapters.items()}
    decisions = {value['case_id']: value for value in compiled.get('directory_decisions', [])}
    required = set(required_native_ids) | set(compiled.get('_reviewed_required_native_table_ids', []))
    records, issues, missing = [], [], []
    for identity, item in items.items():
        selected = explicit.get(identity)
        owners = [chapter_id for chapter_id, values in parts.items() if identity in values]
        state, targets, basis = 'native_carried', owners, []
        if selected:
            index, value = selected
            state = value['action']
            if state == 'reference':
                owner = chapters.get(value['chapter_id'])
                origins = [o for o in item['origins'] if value['case_id'] in (None, o['case_id'])]
                if (owner is None or owner.get('presentation') in {'cover', 'content'}
                        or identity not in parts[owner['id']]
                        or not any(_compatible(o['signature'], owner) for o in origins)):
                    _reject('reference needs the same actual native part in one role/subject/entry-compatible chapter',
                            (index, 'chapter_id'))
                targets = [owner['id']]
            elif owners:
                _reject('this native table is already physically carried; omit its disposition or reference its canonical owner',
                        (index, 'action'))
            elif state == 'exclude':
                if identity in required:
                    _reject('reviewed required native table cannot be excluded; use an existing alternative, complete assignments, '
                            'a valid reference or unresolved; correcting the review needs a superseding review fact', (index, 'action'))
                decision = decisions.get(value['case_id'])
                basis = deepcopy(decision.get('basis_windows', [])) if decision else []
                if not basis or not all(_window_text(source, _window(w, source, 'carrier exclusion basis')).strip() for w in basis):
                    _reject('exclude must cite this table origin case with a nonempty source basis in this M decision',
                            (index, 'case_id'))
        elif not owners:
            missing.append(identity)
            continue
        record = {'table_id': identity, 'state': state, 'chapter_ids': targets,
                  'semantic_confirmed': False, 'source_ref': deepcopy(item['source_ref']),
                  'origins': deepcopy(item['origins'])}
        if selected:
            record['disposition'] = deepcopy(selected[1])
        if basis:
            record['basis_windows'] = basis
            record['basis_text'] = '\n'.join(_window_text(source, _window(w, source, 'carrier exclusion basis')) for w in basis)
        records.append(record)
        if state in {'unresolved', 'exclude'}:
            detail = ('原生载体具名待核，未确认完成' if state == 'unresolved' else '原生载体已明确排除，语义正确性未确认')
            detail += '：' + _location(source_tables.get(identity), source, identity)
            detail += '。可改选已有来源候选或补入完整材料窗口恢复原格；事实不足时保源待核。'
            if basis:
                detail += '\n排除依据沿用本次来源取舍记录：' + '；'.join(
                    f'原件{source.by_id[w["document_id"]]["index"] + 1}物理第{w["page"] + 1}页'
                    f'第{w["start_line"] + 1}–{w["end_line"] + 1}行' for w in basis)
            issues.append({'code': f'outline_directory_material_carrier_{state}', 'detail': detail})
    for identity in sorted(required - set(items)):
        owners = [chapter_id for chapter_id, values in parts.items() if identity in values]
        record = {'table_id': identity, 'state': 'native_carried' if owners else 'unresolved',
                  'chapter_ids': owners, 'semantic_confirmed': False, 'origins': [], 'reviewed_required': True}
        if owners:
            record['source_ref'] = deepcopy(parts[owners[0]][identity]['source_ref'])
        else:
            issues.append({'code': 'outline_directory_material_carrier_unresolved',
                'detail': '已核必需原生载体未实际承载，保源待核：' + _location(source_tables.get(identity), source, identity)
                          + '。该表不在本次来源候选目录内；请依据原核定证据补入完整原表或复核来源，不能普通排除。'})
        records.append(record)
    if missing:
        _reject('unhandled candidate native tables: ' + ', '.join(missing)
                + '; choose an existing alternative, complete assignments, an actual compatible reference, '
                  'an origin-case basis exclusion, or explicit unresolved (reviewed required tables cannot be excluded)',
                code='directory_material_carrier_unhandled')
    compiled['_source_carrier_closure'] = records
    compiled['issues'].extend(issues)
    return compiled
