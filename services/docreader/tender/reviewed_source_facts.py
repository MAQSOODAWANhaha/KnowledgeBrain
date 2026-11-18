"""Explicit owner-reviewed assertions; hashes verify integrity, never semantic truth."""
from copy import deepcopy
import hashlib
import json
from typing import Annotated, Literal

from pydantic import Field, field_validator

from docreader.integrations.artifacts import ArtifactIntegrityError
from docreader.llm.request import ModelConfigurationError, ModelContractError
from docreader.tender.authoring_excerpts import load_source_excerpt, whole_source_excerpts, source_excerpt_key, source_excerpts_text
from docreader.tender.directory_compile import (
    ClassificationPlan, DirectoryPlanRejected, DirectoryPoint, DirectorySubject, DirectoryWindow, _Source, _point, _window, _window_text,
    _table_id, _validate_body_owners, _verbatim_target, _nest, _assign_ids,
    documents_from_frozen,
)
from docreader.tender.directory_choices import validate_reference_exclusions, validate_requirement_exclusions
from docreader.tender.directory_source_groups import apply_source_groups
from docreader.tender.directory_sources import SourceRefs
from docreader.tender.evidence.references import EvidenceRepository
from docreader.tender.source_presentation import (
    SourceDecorationFact, load_decoration, project_decorations, validate_decoration_duties,
)
from docreader.tender.schema import ArtifactRef, AuthoringInput, Contract, Digest, Identity, ResponseTargetKind, SourceExcerptRef, Text


REVIEWED_FACTS_CONTRACT = 'reviewed_source_facts_v1'
REVIEWED_CLASSIFICATION_OPERATION = 'classify_with_reviewed_source_facts_v1'
REVIEWED_BODY_ENTRY_CONTRACT = 'reviewed_body_unique_entry_v1'


class ReviewedFact(Contract):
    label: Text
    source_excerpts: list[SourceExcerptRef] = Field(min_length=1)
    source_text_sha256: Digest
    review_evidence_ref: ArtifactRef
    enterprise_confirmed: Literal[False]
    supersedes: list[Identity] = Field(default_factory=list)

    @field_validator('enterprise_confirmed', mode='before')
    @classmethod
    def no_enterprise_claim(cls, value):
        if value is not False:
            raise ValueError('reviewed source facts must explicitly leave enterprise facts unconfirmed')
        return value


class RequiredNativeFact(ReviewedFact):
    kind: Literal['required_native']
    carrier_heading: DirectoryPoint | None = None


class IndependentBodyFact(ReviewedFact):
    kind: Literal['independent_body_response']
    context_source_excerpts: list[SourceExcerptRef] = Field(default_factory=list)
    source_join: Literal['paragraphs', 'continuation'] = 'paragraphs'
    source_windows: list[DirectoryWindow] = Field(min_length=1)
    subjects: list[DirectorySubject]
    entry: DirectoryPoint | None


class SourceGroupFact(ReviewedFact):
    kind: Literal['source_group']
    heading: DirectoryPoint
    basis_windows: list[DirectoryWindow] = Field(min_length=1)
    member_headings: list[DirectoryPoint] = Field(min_length=1)
    title: Text = Field(max_length=40)


class SubjectLayoutFact(ReviewedFact):
    kind: Literal['subject_layout']
    heading: DirectoryPoint
    response_kind: ResponseTargetKind
    subjects: list[DirectorySubject]


class ReviewedFactsManifest(Contract):
    schema_version: Literal[1]
    contract: Literal['reviewed_source_facts_v1']
    authoring_input: AuthoringInput
    source_projection_digest: Digest
    facts: list[Annotated[RequiredNativeFact | IndependentBodyFact | SourceGroupFact | SubjectLayoutFact | SourceDecorationFact,
                          Field(discriminator='kind')]]

    @field_validator('schema_version', mode='before')
    @classmethod
    def version_one(cls, value):
        if type(value) is not int or value != 1:
            raise ValueError('reviewed facts schema_version must be integer 1')
        return value


class ReviewedBodyDisposition(Contract):
    fact_id: Identity
    scope: Literal['chapter', 'unassigned']
    chapter_id: Identity | None = None


class ReviewedClassificationPlan(ClassificationPlan):
    reviewed_body_dispositions: list[ReviewedBodyDisposition] = Field(default_factory=list)


def _canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':'))


def _digest(value):
    return hashlib.sha256(_canonical(value).encode()).hexdigest()


def _body_scope_signature(fact):
    """Coverage is independent of fragment order, partitioning and review labels."""
    roles = [s['role'] for s in fact['subjects']]
    if len(roles) != len(set(roles)):
        raise ArtifactIntegrityError('independent body scope cannot repeat an indistinguishable role')
    ranges = {}
    for excerpt in fact['source_excerpts']:
        reference = excerpt['source_ref']
        for selected in excerpt['selectors']:
            key = (reference['source_index_ref']['sha256'], reference['unit_id'], selected['source_range_id'])
            ranges.setdefault(key, []).append((selected['start'], selected['end']))
    coverage = []
    for key, spans in sorted(ranges.items()):
        merged = []
        for start, end in sorted(spans):
            if merged and start <= merged[-1][1]:
                merged[-1] = (merged[-1][0], max(merged[-1][1], end))
            else:
                merged.append((start, end))
        coverage.append((key, merged))
    return _canonical((coverage, sorted(roles), fact['entry']))


def _manifest(artifacts, reference):
    raw = artifacts.read_json(ArtifactRef.model_validate(reference).model_dump())
    snapshot = raw.get('authoring_input') if isinstance(raw, dict) else None
    if not isinstance(snapshot, dict) or set(snapshot) != set(AuthoringInput.model_fields):
        raise ArtifactIntegrityError('reviewed manifest requires the complete authoring input')
    value = ReviewedFactsManifest.model_validate(raw).model_dump()
    facts = []
    for raw in value['facts']:
        artifacts.read_bytes(raw['review_evidence_ref'])
        # IDs include the complete source snapshot and review; target IDs are never evidence.
        identity = {key: value[key] for key in ('contract', 'authoring_input', 'source_projection_digest')}
        canonical_fact = {k: v for k, v in raw.items() if not (k == 'source_join' and v == 'paragraphs')}
        facts.append({**raw, 'fact_id': 'reviewed-fact-' + _digest({**identity, 'fact': canonical_fact})})
    ids = {fact['fact_id'] for fact in facts}
    if len(ids) != len(facts) or any(len(f['supersedes']) != len(set(f['supersedes'])) or ids.intersection(f['supersedes']) for f in facts):
        raise ArtifactIntegrityError('reviewed facts must be unique and cannot supersede another active fact')
    return {**value, 'facts': facts}


def validate_fact_successor(artifacts, previous_ref, new_ref):
    """An omitted ref cannot erase an earlier review; correction needs fresh review evidence."""
    if previous_ref is None:
        if new_ref is not None:
            _manifest(artifacts, new_ref)
        return
    previous = _manifest(artifacts, previous_ref)
    if new_ref is None:
        raise ArtifactIntegrityError('reviewed source facts cannot be omitted; retain or explicitly supersede them')
    current = _manifest(artifacts, new_ref)
    previous_ids = {f['fact_id']: f for f in previous['facts']}
    current_ids = {f['fact_id'] for f in current['facts']}
    replacements = {}
    for fact in current['facts']:
        if fact['fact_id'] in previous_ids:
            continue
        for old in fact['supersedes']:
            if old not in previous_ids or old in replacements or old in current_ids:
                raise ArtifactIntegrityError('supersedes must identify one removed predecessor fact exactly once')
            if fact['review_evidence_ref']['sha256'] == previous_ids[old]['review_evidence_ref']['sha256']:
                raise ArtifactIntegrityError('superseding a reviewed fact requires new review evidence')
            replacements[old] = fact['fact_id']
    if set(previous_ids) - current_ids - set(replacements):
        raise ArtifactIntegrityError('successor must retain or explicitly supersede every reviewed source fact')


def _subjects(source, subjects, document_ids):
    seen = set()
    for subject in subjects:
        window = _window(subject['source'], source, 'reviewed subject source')
        if window[0] not in document_ids or subject['role'] not in _window_text(source, window):
            raise ArtifactIntegrityError('reviewed subject role must remain an exact fragment of its source document')
        identity = (_canonical(subject['source']), subject['role'])
        if identity in seen:
            raise ArtifactIntegrityError('reviewed subject selections must not repeat')
        seen.add(identity)


def load_reviewed_facts(artifacts, snapshot, documents, manifest_ref):
    if manifest_ref is None:
        return ()
    manifest = _manifest(artifacts, manifest_ref)
    snapshot = AuthoringInput.model_validate(snapshot).model_dump()
    if manifest['authoring_input'] != snapshot:
        raise ArtifactIntegrityError('reviewed facts belong to another complete authoring input')
    if manifest['source_projection_digest'] != SourceRefs(documents).digest:
        raise ArtifactIntegrityError('reviewed facts source projection changed')
    if snapshot['frozen_ref'] is None:
        raise ArtifactIntegrityError('reviewed facts require immutable registered source documents')
    frozen = artifacts.read_json(snapshot['frozen_ref'])
    # Do not accept an alternate line projection with an equally self-consistent manifest.
    if documents_from_frozen(artifacts, frozen) != documents:
        raise ArtifactIntegrityError('reviewed facts must use the current frozen document projection')
    registered, repository = {}, EvidenceRepository(artifacts)
    for record in frozen['documents']:
        ref = ArtifactRef.model_validate(record['source_index_ref']).model_dump()
        index = repository.source_index(ref)
        identity = {k: record[k] for k in ('document_id', 'document_version', 'original_ref')}
        if index.document != identity:
            raise ArtifactIntegrityError('source index identity differs from its frozen document')
        artifacts.read_bytes(ArtifactRef.model_validate(identity['original_ref']).model_dump())
        registered[_canonical(ref)] = index
    source, result, exclusive = _Source(documents), [], set()
    decorations = {}
    for raw in manifest['facts']:
        if raw['kind'] == 'source_decoration':
            result.append(load_decoration(artifacts, source, frozen, raw, decorations))
            continue
        fact, texts, document_ids = deepcopy(raw), [], set()
        selected = set()
        for excerpt in fact['source_excerpts']:
            index = registered.get(_canonical(excerpt['source_ref']['source_index_ref']))
            if index is None:
                raise ArtifactIntegrityError('reviewed excerpt source index is not a current frozen document')
            document_ids.add(index.document['document_id'])
            loaded = load_source_excerpt(artifacts, excerpt, repository=repository)
            for part in loaded['ranges']:
                identity = (index.document['document_id'], part['source_range_id'], part['start'], part['end'])
                if identity in selected:
                    raise ArtifactIntegrityError('reviewed source excerpts repeat an exact interval')
                selected.add(identity)
            texts.append(loaded['text'])
        text = (source_excerpts_text(artifacts, fact['source_excerpts'], repository=repository,
                                    source_join=fact.get('source_join', 'paragraphs'))
                if fact['kind'] == 'independent_body_response' else '\n'.join(texts))
        context_texts = []
        for excerpt in fact.get('context_source_excerpts', []):
            index = registered.get(_canonical(excerpt['source_ref']['source_index_ref']))
            if index is None or index.document['document_id'] not in document_ids:
                raise ArtifactIntegrityError('reviewed body context must use its current frozen source document')
            context_texts.append(source_excerpts_text(artifacts, [excerpt], repository=repository))
        if fact['kind'] == 'independent_body_response':
            fact['_context_text'] = '\n'.join(context_texts)
        if hashlib.sha256(text.encode()).hexdigest() != fact['source_text_sha256']:
            raise ArtifactIntegrityError('reviewed native source text digest changed')
        fact['_source_text'] = text
        for key in ('heading', 'entry', 'carrier_heading'):
            if fact.get(key) is not None:
                point = _point(fact[key], source, 'reviewed ' + key)
                if point[0] not in document_ids or not source.line(*point).strip():
                    raise ArtifactIntegrityError('reviewed anchor needs nonempty source in its excerpt document')
        for key in ('source_windows', 'basis_windows'):
            windows = [_window(window, source, 'reviewed ' + key) for window in fact.get(key, [])]
            if any(w[0] not in document_ids or not _window_text(source, w).strip() for w in windows):
                raise ArtifactIntegrityError('reviewed windows need nonempty source in their excerpt document')
        if 'subjects' in fact:
            _subjects(source, fact['subjects'], document_ids)
        if fact['kind'] == 'required_native':
            refs = { _canonical(e['source_ref']) for e in fact['source_excerpts'] }
            if len(refs) != 1:
                raise ArtifactIntegrityError('required_native must select one complete native part')
            reference = fact['source_excerpts'][0]['source_ref']
            index = registered[_canonical(reference['source_index_ref'])]
            unit = next(u for u in index.raw['units'] if u['unit_id'] == reference['unit_id'])
            whole = whole_source_excerpts(artifacts, reference, repository=repository)
            if not unit.get('grid') or [source_excerpt_key(e) for e in whole] != [source_excerpt_key(e) for e in fact['source_excerpts']]:
                raise ArtifactIntegrityError('required_native must preserve the complete native grid part')
            fact['_table_id'] = _table_id({'source_ref': reference})
        elif fact['kind'] == 'source_group':
            from docreader.tender.directory_titles import business_title
            business_title(fact['title'])
            heading = fact['heading']
            if not any(w['document_id'] == heading['document_id'] and w['page'] == heading['page']
                       and w['start_line'] <= heading['line'] <= w['end_line'] for w in fact['basis_windows']):
                raise ArtifactIntegrityError('reviewed group basis must include its exact heading')
            seen = set()
            for member in fact['member_headings']:
                p = _point(member, source, 'reviewed group member')
                if p in seen or p[0] != heading['document_id'] or not source.line(*p).strip():
                    raise ArtifactIntegrityError('reviewed group members need unique same-document source anchors')
                seen.add(p)
        if fact['kind'] in {'subject_layout', 'source_group', 'required_native'}:
            key = (fact['kind'], _canonical(fact.get('heading', fact.get('_table_id'))))
            if key in exclusive:
                raise ArtifactIntegrityError('conflicting reviewed facts need an explicit superseding review')
            exclusive.add(key)
        elif fact['kind'] == 'independent_body_response':
            key = (fact['kind'], _body_scope_signature(fact))
            if key in exclusive:
                raise ArtifactIntegrityError('duplicate independent body facts have the same source coverage and instance scope')
            exclusive.add(key)
        result.append(fact)
    validate_decoration_duties(artifacts, repository, result, registered.values(), source)
    return tuple(result)


def _pending(compiled, fact, detail):
    text = f'已核来源“{fact["label"]}”待核：{detail}。企业事实未确认。请对照原核定来源补充核定。'
    if fact['kind'] == 'independent_body_response':
        text += '\n' + fact['_source_text']
        if fact.get('_context_text'):
            text += '\n来源上下文（不作为企业应答）：\n' + fact['_context_text']
    issue = {'code': 'outline_directory_reviewed_fact_pending', 'detail': text, 'rule_refs': []}
    if issue not in compiled['issues']:
        compiled['issues'].append(issue)


def _application(compiled, fact, state, **fields):
    record = {'fact_id': fact['fact_id'], 'kind': fact['kind'], 'state': state, 'enterprise_confirmed': False, **deepcopy(fields)}
    if fact.get('source_join', 'paragraphs') != 'paragraphs':
        record['source_join'] = fact['source_join']
    records = compiled.setdefault('_reviewed_source_fact_applications', [])
    records[:] = [r for r in records if r['fact_id'] != fact['fact_id']]
    records.append(record)


def reviewed_material_summary(facts):
    """Host-owned layout/group values are applied locally, never re-transcribed by M."""
    return [{'kind': f['kind'], 'label': f['label'], **({'table_id': f['_table_id'], 'carrier_heading': deepcopy(f['carrier_heading'])} if f['kind'] == 'required_native'
            else {'heading': deepcopy(f['heading'])})}
            for f in facts if f['kind'] in {'required_native', 'source_group', 'subject_layout'}]


def apply_reviewed_material_facts(compiled, documents, facts):
    by_heading = {}
    for chapter in compiled['chapters']:
        if not chapter.get('unplaced'):
            by_heading.setdefault(_canonical(chapter['heading']), []).append(chapter)
    known = {c['id']: c for c in compiled['chapters']}
    for fact in facts:
        kind = fact['kind']
        if kind == 'subject_layout':
            matches = by_heading.get(_canonical(fact['heading']), [])
            if len(matches) != 1 or matches[0].get('response_kind', 'form') != fact['response_kind']:
                _pending(compiled, fact, '准确来源章缺失、不唯一或响应职责改变，未更改正文与原式')
                _application(compiled, fact, 'unresolved')
                continue
            chapter = matches[0]
            before = deepcopy(chapter.get('subjects', []))
            chapter['subjects'] = deepcopy(fact['subjects'])
            _application(compiled, fact, 'applied', chapter_ids=[chapter['id']], previous_subjects=before,
                         subjects=fact['subjects'])
            issue = {'code': 'outline_directory_source_decision', 'detail':
                f'已核来源“{fact["label"]}”采用明确主体布局：' + ('、'.join(s['role'] for s in fact['subjects']) or '整份单实例')
                + '。仅覆盖主体布局，原正文、条件及原式保留；企业事实未确认。'}
            if issue not in compiled['issues']:
                compiled['issues'].append(issue)
        elif kind == 'source_group':
            members, retained, pending = [], [], []
            for heading in fact['member_headings']:
                candidates = by_heading.get(_canonical(heading), [])
                chapter = candidates[0] if len(candidates) == 1 else None
                if (chapter is None or chapter.get('parent_id') in known
                        or chapter.get('presentation') in {'cover', 'content'}
                        or (chapter.get('group') and chapter.get('group_heading') != fact['heading'])):
                    pending.append(heading)
                elif chapter.get('group_heading') == fact['heading'] and chapter.get('group'):
                    # Preserve an already source-bound group; complete only missing members.
                    retained.append(chapter['id'])
                    continue
                else:
                    members.append(chapter['id'])
            if members:
                # Existing helper forbids an already-known group anchor. A narrow shadow
                # validates/constructs the missing members without touching adopted members.
                shadow = deepcopy(compiled)
                shadow['chapters'] = [c for c in shadow['chapters'] if not
                    (c.get('group') and c.get('group_heading') == fact['heading'])]
                selection = {k: deepcopy(fact[k]) for k in ('heading', 'basis_windows', 'title')}
                apply_source_groups(shadow, documents, [{**selection, 'chapter_ids': members}])
                updated = {c['id']: c for c in shadow['chapters']}
                for identity in members:
                    for field in ('group', 'group_heading', 'group_display_title', 'group_basis_windows'):
                        known[identity][field] = deepcopy(updated[identity][field])
                compiled['issues'].extend(i for i in shadow['issues'] if i not in compiled['issues'])
                compiled.setdefault('_source_groups', []).extend(shadow.get('_source_groups', [])[-1:])
            if pending:
                locations = '；'.join(f'第{p["page"] + 1}页第{p["line"] + 1}行' for p in pending)
                _pending(compiled, fact, f'分册“{fact["title"]}”成员来源锚{locations}未找到可用root，已匹配成员保留')
            _application(compiled, fact, 'unresolved' if pending else 'applied', chapter_ids=[*retained, *members], pending_headings=pending)
    validate_reference_exclusions(compiled)
    _validate_body_owners([c for c in compiled['chapters'] if not c.get('unplaced')])
    return compiled


def bind_reviewed_carrier_results(compiled, facts):
    from docreader.tender.directory_material_closure import native_parts
    for fact in facts:
        if fact['kind'] != 'required_native' or fact['carrier_heading'] is None:
            continue
        matches = [c for c in compiled['chapters'] if not c.get('unplaced') and c['heading'] == fact['carrier_heading']]
        valid = (len(matches) == 1 and matches[0].get('presentation') not in {'cover', 'content'}
                 and fact['_table_id'] in native_parts(matches[0]))
        if not valid:
            _pending(compiled, fact, '原表未在已核职责的准确来源章承载；可改选已有来源候选或完整材料窗口，不能用其他章物理出现抵销')
        for record in compiled.get('_source_carrier_closure', []):
            if record['table_id'] == fact['_table_id']:
                record.update(reviewed_fact_id=fact['fact_id'], reviewed_carrier_heading=deepcopy(fact['carrier_heading']),
                    reviewed_owner_state='source_bound' if valid else 'unresolved')
        _application(compiled, fact, 'source_bound' if valid else 'unresolved',
                     chapter_ids=[matches[0]['id']] if valid else [], carrier_heading=fact['carrier_heading'])


def _compatible_body(fact, chapter):
    return (not chapter.get('unplaced') and chapter.get('presentation') not in {'cover', 'content'}
            and chapter.get('response_kind', 'form') in {'narrative', 'form', 'attachment'}
            and chapter.get('subjects', []) == fact['subjects'] and chapter.get('entry') == fact['entry'])


def reviewed_body_summary(facts):
    return [{'fact_id': f['fact_id'], 'kind': f['kind'], 'label': f['label'],
             'source_windows': deepcopy(f['source_windows']), 'body': f['_source_text'],
             'context': f['_context_text'], 'subjects': deepcopy(f['subjects']), 'entry': deepcopy(f['entry']),
             **({'source_join': f['source_join']} if f.get('source_join', 'paragraphs') != 'paragraphs' else {})}
            for f in facts if f['kind'] == 'independent_body_response']


def reviewed_classification_view(compiled, facts):
    """Exact same-source legacy cases become provenance, retaining their original authority."""
    shadow, origins = deepcopy(compiled), {}
    active = {i.get('requirement_case_id') for i in compiled['issues']
              if i['code'] == 'outline_directory_requirement_unassigned'}
    def coverage(windows):
        return {(w['document_id'], w['page'], line) for w in windows
                for line in range(w['start_line'], w['end_line'] + 1)}
    cases = {c['id']: c for c in compiled.get('_requirement_cases', []) if c['id'] in active}
    covered, by_point = {}, {}
    for identity, case in cases.items():
        covered[identity] = coverage(case['source_windows'])
        for point in covered[identity]:
            by_point.setdefault(point, set()).add(identity)
    for fact in facts:
        if fact['kind'] == 'independent_body_response':
            wanted = coverage(fact['source_windows'])
            touched = set().union(*(by_point.get(point, set()) for point in wanted)) if wanted else set()
            selected = {identity for identity in touched if covered[identity] <= wanted}
            complete = set().union(*(covered[identity] for identity in selected)) if selected else set()
            origins[fact['fact_id']] = [deepcopy(c) for identity, c in cases.items()
                if identity in selected] if complete == wanted else []
    linked = {c['id'] for cases in origins.values() for c in cases}
    shadow['issues'] = [i for i in shadow['issues'] if not
        (i['code'] == 'outline_directory_requirement_unassigned' and i.get('requirement_case_id') in linked)]
    return shadow, origins


def validate_reviewed_body_dispositions(compiled, documents, dispositions, facts):
    required = {f['fact_id']: f for f in facts if f['kind'] == 'independent_body_response'}
    known = {c['id']: c for c in compiled['chapters']}
    seen = set()
    for decision in dispositions:
        identity = decision['fact_id']
        fact, scope, owner = required.get(identity), decision['scope'], decision['chapter_id']
        detail = None
        if fact is None or identity in seen:
            detail = 'choose each supplied reviewed body fact exactly once'
        elif ((scope == 'unassigned' and owner is not None) or (scope == 'chapter'
                and (owner not in known or not _compatible_body(fact, known[owner])))):
            detail = 'reviewed body needs one compatible narrative/form/attachment canonical chapter or unassigned with no owner'
        if detail:
            raise ModelContractError([{'path': ['reviewed_body_dispositions'], 'code': 'reviewed_body_fact_invalid', 'detail': detail}])
        if scope == 'chapter':
            validate_requirement_exclusions(compiled, [{**w, 'scope': scope} for w in fact['source_windows']])
        seen.add(identity)
    if seen != set(required):
        raise ModelContractError([{'path': ['reviewed_body_dispositions'], 'code': 'reviewed_body_fact_incomplete',
            'detail': 'retain every reviewed body fact: select one canonical chapter or explicit unassigned; correcting review needs superseding evidence'}])


def apply_reviewed_body_dispositions(compiled, documents, dispositions, facts, *, entry_contract=None):
    validate_reviewed_body_dispositions(compiled, documents, dispositions, facts)
    if entry_contract not in (None, REVIEWED_BODY_ENTRY_CONTRACT):
        raise ModelConfigurationError('unsupported reviewed body entry contract')
    required = {f['fact_id']: f for f in facts if f['kind'] == 'independent_body_response'}
    known = {c['id']: c for c in compiled['chapters']}
    for decision in dispositions:
        fact = required[decision['fact_id']]
        owner = None if decision['scope'] == 'unassigned' else known[decision['chapter_id']]
        audit = ({'entry_contract': entry_contract, 'input_disposition': deepcopy(decision)}
                 if entry_contract is not None else {})
        pending_detail = '独立正文尚未选定兼容章；可在本次来源裁定选择方案章或保留未决'
        if (decision['scope'] == 'unassigned' and entry_contract == REVIEWED_BODY_ENTRY_CONTRACT
                and fact['entry'] is not None):
            candidates = [chapter for chapter in compiled['chapters'] if _compatible_body(fact, chapter)]
            if len(candidates) == 1:
                try:
                    validate_requirement_exclusions(compiled, [{**w, 'scope': 'chapter'} for w in fact['source_windows']])
                except DirectoryPlanRejected as error:
                    audit['entry_resolution_blocker'] = error.detail
                    pending_detail = '独立正文与已排除提交来源冲突；请先核定来源取舍，或保留未决'
                else:
                    owner = candidates[0]
                    audit['host_resolution'] = {'kind': 'unique_entry', 'chapter_id': owner['id']}
        if owner is None:
            _pending(compiled, fact, pending_detail)
            _application(compiled, fact, 'unresolved', source_excerpts=fact['source_excerpts'],
                         context_source_excerpts=fact['context_source_excerpts'], **audit)
            continue
        text = fact['_source_text']
        # A dedicated final target preserves native authority without inferring
        # legacy provenance from equal purpose strings or changing old case text.
        _application(compiled, fact, 'body_bound', chapter_ids=[owner['id']], source_excerpts=fact['source_excerpts'],
                     context_source_excerpts=fact['context_source_excerpts'], **audit)
        issue = {'code': 'outline_directory_source_decision', 'detail':
            f'已核来源“{fact["label"]}”的独立正文采用“{owner["title"]}”；企业事实未确认。\n完整来源正文：\n' + text
            + ('\n来源名称及评审上下文（不作为企业应答）：\n' + fact['_context_text'] if fact['_context_text'] else '')}
        if issue not in compiled['issues']:
            compiled['issues'].append(issue)
    return compiled


def apply_reviewed_classification(compiled, documents, plan, facts, *, entry_contract=None):
    from docreader.tender.directory_compile import apply_classification
    from docreader.tender.directory_adjudication import apply_requirement_adjudications
    shadow, origins = reviewed_classification_view(compiled, facts)
    if plan is None:
        dispositions = [{'fact_id': f['fact_id'], 'scope': 'unassigned', 'chapter_id': None} for f in facts]
    else:
        apply_classification(shadow, documents, plan['items'])
        apply_requirement_adjudications(shadow, documents, plan['requirement_adjudications'])
        dispositions = plan['reviewed_body_dispositions']
    apply_reviewed_body_dispositions(shadow, documents, dispositions, facts,
                                     entry_contract=entry_contract)
    bound = {r['fact_id'] for r in shadow.get('_reviewed_source_fact_applications', []) if r['state'] == 'body_bound'}
    pending_ids = {c['id'] for identity, cases in origins.items()
                   if identity not in bound for c in cases}
    shadow['issues'].extend(deepcopy(i) for i in compiled['issues'] if
        i['code'] == 'outline_directory_requirement_unassigned' and i.get('requirement_case_id') in pending_ids)
    for record in shadow.get('_reviewed_source_fact_applications', []):
        cases = origins.get(record['fact_id'], [])
        if cases:
            record['origin_requirement_cases'] = cases
            if record['state'] == 'body_bound':
                shadow['issues'].append({'code': 'outline_directory_source_decision',
                    'detail': '对应旧layout未决来源由已核native独立正文承载；旧case原文保留在来源记录，未作为正文权威复制。企业事实未确认。'})
    compiled.clear()
    compiled.update(shadow)
    return compiled


def bind_reviewed_outline_facts(outline, compiled, facts):
    """Bind actual final targets by the same host source-node map used by compilation."""
    nodes = {}
    _assign_ids(_nest(compiled['chapters'], source_nodes=nodes))
    def walk(chapters):
        for c in chapters:
            yield c
            yield from walk(c['children'])
    chapters = {c['chapter_id']: c for c in walk(outline['chapters'])}
    applications = {r['fact_id']: r for r in compiled.get('_reviewed_source_fact_applications', [])}
    for fact in facts:
        record = applications.get(fact['fact_id'], {})
        if record.get('state') != 'body_bound':
            continue
        source_id, = record['chapter_ids']
        mapped = nodes.get(source_id)
        chapter = chapters.get(mapped['chapter_id']) if mapped else None
        if chapter is None:
            _pending(outline, fact, '无法唯一定位最终正文，请核定实际位置')
            continue
        target_ids = []
        expected_sources = [source_excerpt_key(e) for e in fact['source_excerpts']]
        for subject in [s['role'] for s in fact['subjects']] or [None]:
            matched = [t for t in chapter['targets'] if t['kind'] == 'narrative' and t['purpose'] == fact['_source_text']
                and t.get('subject') == subject and [source_excerpt_key(e) for e in t.get('source_excerpts', [])] == expected_sources]
            if len(matched) == 1:
                target_ids.append(matched[0]['target_id'])
                continue
            target = _verbatim_target('narrative', fact['_source_text'])
            target.update(target_id=f'{chapter["chapter_id"]}:target-{len(chapter["targets"]) + 1}',
                          source_excerpts=deepcopy(fact['source_excerpts']))
            if subject is not None:
                target['subject'] = subject
            chapter['targets'].append(target)
            target_ids.append(target['target_id'])
        record['target_ids'] = target_ids
    project_decorations(outline, compiled, facts)
    _bind_all_fact_applications(outline, compiled, facts, nodes, chapters)
    return outline


def _bind_all_fact_applications(outline, compiled, facts, nodes, chapters):
    source_chapters = {c['id']: c for c in compiled['chapters']}
    parents = {}
    def index_parents(entries, parent=None):
        for chapter in entries:
            parents[chapter['chapter_id']] = parent
            index_parents(chapter['children'], chapter['chapter_id'])
    index_parents(outline['chapters'])
    def volume(identity):
        while identity is not None:
            if chapters[identity].get('is_volume'):
                return identity
            identity = parents[identity]
        return None
    applications = {r['fact_id']: r for r in compiled.get('_reviewed_source_fact_applications', [])}
    for fact in facts:
        if fact['kind'] == 'source_decoration':
            continue
        record = applications.get(fact['fact_id'])
        if record is None:
            record = {'fact_id': fact['fact_id'], 'kind': fact['kind'], 'state': 'unresolved', 'enterprise_confirmed': False}
            compiled.setdefault('_reviewed_source_fact_applications', []).append(record)
        record.update(label=fact['label'], source_excerpts=deepcopy(fact['source_excerpts']),
            review_evidence_ref=deepcopy(fact['review_evidence_ref']), source_text_sha256=fact['source_text_sha256'])
        source_ids = record.get('chapter_ids', [])
        final = [nodes[identity]['chapter_id'] for identity in source_ids if identity in nodes
                 and nodes[identity]['chapter_id'] in chapters]
        valid = bool(final) and len(final) == len(source_ids) and record['state'] != 'unresolved'
        targets = [t for identity in final for t in chapters[identity]['targets']]
        if fact['kind'] == 'required_native':
            permitted = final if fact['carrier_heading'] is not None else list(chapters)
            bound = [(identity, t) for identity in permitted for t in chapters[identity]['targets']
                if any(_table_id(part) == fact['_table_id'] for part in t.get('template_refs', []))]
            valid = bool(bound) and (valid if fact['carrier_heading'] is not None else True)
            final = list(dict.fromkeys(identity for identity, _ in bound))
            targets = [t for _, t in bound]
            if valid and fact['carrier_heading'] is None:
                record['state'] = 'native_carried'
        elif fact['kind'] == 'subject_layout':
            expected = {s['role'] for s in fact['subjects']}
            actual = {t['subject'] for t in targets if t.get('subject') is not None}
            valid = valid and bool(targets) and actual == expected
        elif fact['kind'] == 'source_group':
            groups = {volume(identity) for identity in final}
            valid = valid and None not in groups and len(groups) == 1 and all(
                source_chapters[identity].get('group_heading') == fact['heading'] for identity in source_ids)
            record['volume_chapter_ids'] = sorted(g for g in groups if g is not None)
            def subtree_targets(chapter):
                yield from chapter['targets']
                for child in chapter['children']:
                    yield from subtree_targets(child)
            targets = [t for identity in final for t in subtree_targets(chapters[identity])]
        else:
            requested = set(record.get('target_ids', []))
            targets = [t for t in targets if t['target_id'] in requested]
            valid = valid and bool(requested) and len(targets) == len(requested) and all(
                t['purpose'] == fact['_source_text'] and [source_excerpt_key(e) for e in t.get('source_excerpts', [])]
                == [source_excerpt_key(e) for e in fact['source_excerpts']] for t in targets)
        record['final_chapter_ids'] = list(dict.fromkeys(final))
        record['target_ids'] = list(dict.fromkeys(t['target_id'] for t in targets))
        if not valid:
            record['state'] = 'unresolved'
            _pending(outline, fact, '最终章节或响应位置待确认，请核定实际位置')
