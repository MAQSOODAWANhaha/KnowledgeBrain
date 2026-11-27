"""Host-owned exact body bindings over frozen native cells and reader lines."""
from copy import deepcopy
import hashlib
from typing import Literal, cast

from pydantic import Field

from docreader.integrations.artifacts import ArtifactIntegrityError
from docreader.tender.authoring_excerpts import source_excerpts_text
from docreader.tender.evidence.references import EvidenceRepository
from docreader.tender.schema import (
    ArtifactRef, AuthoringInput, Contract, Digest, Identity, NonnegativeInt,
    ResponseTarget, SOURCE_BODY_CONTRACT, SourceExcerptRef,
)
from docreader.tender.status import DocumentRole


class LayoutSpan(Contract):
    document_id: Identity
    page: NonnegativeInt
    line: NonnegativeInt
    start: NonnegativeInt
    end: NonnegativeInt
    line_sha256: Digest


class BodyPart(Contract):
    source_excerpt: SourceExcerptRef | None = None
    layout_span: LayoutSpan | None = None
    join: Literal['paragraphs', 'continuation'] = 'paragraphs'


class SourceContextGroup(Contract):
    source_excerpts: list[SourceExcerptRef] = Field(min_length=1)
    source_join: Literal['paragraphs', 'continuation'] = 'paragraphs'


class BodyBinding(Contract):
    contract: Literal['source_bound_native_body_v1']
    authoring_input: AuthoringInput
    target_id: Identity
    kind: Identity
    subject: Identity | None = None
    parts: list[BodyPart] = Field(min_length=1)
    context_source_excerpts: list[SourceExcerptRef] = Field(default_factory=list)
    context_groups: list[SourceContextGroup] | None = None
    source_projection_ref: ArtifactRef | None = None


def _input(snapshot):
    return AuthoringInput.model_validate(snapshot).model_dump(exclude_none=True)


def _documents(artifacts, snapshot, *, project_context=False):
    frozen = snapshot.get('frozen_ref')
    if not frozen:
        raise ArtifactIntegrityError('exact body requires its frozen source documents')
    value = artifacts.read_json(frozen)
    if project_context and (reason := project_context_blocker(value)):
        raise ArtifactIntegrityError(reason)
    documents = value['documents']
    by_id = {d['document_id']: d for d in documents}
    if len(by_id) != len(documents):
        raise ArtifactIntegrityError('duplicate frozen document identity')
    return by_id


def save_source_projection(artifacts, snapshot, documents):
    """Save the already-read full page projection once; never infer native cells."""
    frozen = _documents(artifacts, snapshot)
    if len(documents) != len(frozen) or {d['document_id'] for d in documents} != set(frozen):
        raise ArtifactIntegrityError('reader projection must cover the exact frozen documents')
    projected = []
    for document in documents:
        pages = document['pages']
        if not isinstance(pages, list) or any(not isinstance(page, str) for page in pages):
            raise ArtifactIntegrityError('invalid frozen reader pages')
        source = frozen[document['document_id']]
        projected.append({key: deepcopy(source[key]) for key in
                          ('document_id', 'original_ref', 'source_index_ref')})
        projected[-1]['pages'] = deepcopy(pages)
    return artifacts.put_json({'contract': 'frozen_layout_lines_v1',
                               'authoring_input': _input(snapshot), 'documents': projected})


def _native_sources(artifacts, excerpts, documents, repository):
    for excerpt in excerpts:
        reference = excerpt['source_ref']['source_index_ref']
        index = repository.source_index(reference)
        source = documents.get(index.document['document_id'])
        if (source is None or source.get('source_index_ref') != reference
                or source['original_ref'] != index.document['original_ref']):
            raise ArtifactIntegrityError('body source is outside the current frozen documents')
    return source_excerpts_text(artifacts, excerpts, repository=repository)


def _materialize(artifacts, snapshot, binding, repository, cache=None):
    cache = {} if cache is None else cache
    frozen_key = ('frozen', (snapshot.get('frozen_ref') or {}).get('sha256'))
    if frozen_key not in cache:
        cache[frozen_key] = _documents(artifacts, snapshot)
    documents = cache[frozen_key]
    projection = None
    if binding.get('source_projection_ref'):
        projection_key = ('projection', binding['source_projection_ref']['sha256'])
        if projection_key not in cache:
            cache[projection_key] = artifacts.read_json(binding['source_projection_ref'])
        projection = cache[projection_key]
        if projection.get('contract') != 'frozen_layout_lines_v1' or projection.get('authoring_input') != _input(snapshot):
            raise ArtifactIntegrityError('body reader projection belongs to another frozen input')
        projected = projection.get('documents', [])
        if len(projected) != len(documents) or {d['document_id'] for d in projected} != set(documents):
            raise ArtifactIntegrityError('body reader projection lost a frozen document')
        for source in projected:
            if any(source.get(key) != documents[source['document_id']].get(key)
                   for key in ('original_ref', 'source_index_ref')):
                raise ArtifactIntegrityError('body reader projection source identity changed')
        projection = {d['document_id']: d for d in projected}
    text, excerpts, occupied, positions = '', [], {}, {}
    previous_document, previous_page, previous_domain = None, None, None
    for ordinal, part in enumerate(binding['parts']):
        native, layout = part.get('source_excerpt'), part.get('layout_span')
        if (native is None) == (layout is None) or ordinal == 0 and part['join'] != 'paragraphs':
            raise ArtifactIntegrityError('body part must select one source and an explicit join')
        if native is not None:
            value = _native_sources(artifacts, [native], documents, repository)
            index = repository.source_index(native['source_ref']['source_index_ref'])
            document_id = index.document['document_id']
            unit_order = next(i for i, unit in enumerate(index.raw['units'])
                              if unit['unit_id'] == native['source_ref']['unit_id'])
            unit = index.raw['units'][unit_order]
            bases = [index.by_id[s['source_range_id']] for s in native['selectors']]
            physical_pages = {(base.get('physical_locator') or {}).get('page_ordinal') for base in bases}
            physical_pages.discard(None)
            page = (next(iter(physical_pages)) if len(physical_pages) == 1 else None)
            if not physical_pages:
                page = next((loc['page_ordinal'] for loc in (unit.get('physical_locator'), unit.get('locator'))
                             if isinstance(loc, dict) and type(loc.get('page_ordinal')) is int), None)
            ordered = [(unit_order, index.by_id[s['source_range_id']].get('row_start', -1), s['start'], s['end'])
                       for s in native['selectors']]
            domain = ('native', document_id)
            excerpts.append(native)
            intervals = [(('native', native['source_ref']['source_index_ref']['sha256'],
                           native['source_ref']['unit_id'], s['source_range_id']), s['start'], s['end'])
                         for s in native['selectors']]
        else:
            if projection is None or layout['document_id'] not in projection:
                raise ArtifactIntegrityError('layout body requires its frozen reader projection')
            try:
                line = projection[layout['document_id']]['pages'][layout['page']].splitlines()[layout['line']]
            except (IndexError, KeyError, TypeError):
                raise ArtifactIntegrityError('body layout line is outside its frozen projection') from None
            first, last = layout['start'], layout['end']
            if hashlib.sha256(line.encode()).hexdigest() != layout['line_sha256'] or not 0 <= first < last <= len(line):
                raise ArtifactIntegrityError('body layout characters do not match the frozen line')
            value = line[first:last]
            document_id = layout['document_id']
            page = layout['page']
            domain = ('layout', document_id)
            ordered = [(layout['page'], layout['line'], first, last)]
            intervals = [(('layout', layout['document_id'], layout['page'], layout['line']), first, last)]
        if part['join'] == 'continuation' and document_id != previous_document:
            raise ArtifactIntegrityError('body continuation must stay in one frozen document')
        if document_id == previous_document:
            if type(page) is int and type(previous_page) is int and page < previous_page:
                raise ArtifactIntegrityError('body source parts cannot reverse physical pages')
            if (part['join'] == 'continuation' and domain[0] != cast(tuple, previous_domain)[0]
                    and (type(page) is not int or type(previous_page) is not int or page <= previous_page)):
                raise ArtifactIntegrityError('mixed source continuation requires proven forward physical pages')
        for first_position, second_position, first, last in ordered:
            start_position = (first_position, second_position, first)
            if domain in positions and start_position < positions[domain]:
                raise ArtifactIntegrityError('body source parts must run forward without overlap')
            positions[domain] = (first_position, second_position, last)
        previous_document, previous_page, previous_domain = document_id, page, domain
        for key, first, last in intervals:
            if first >= last or any(a < last and first < b for a, b in occupied.get(key, [])):
                raise ArtifactIntegrityError('body source intervals must be nonempty and disjoint')
            occupied.setdefault(key, []).append((first, last))
        text += ('\n' if ordinal and part['join'] == 'paragraphs' else '') + value
    context = binding['context_source_excerpts']
    _native_sources(artifacts, context, documents, repository)
    groups = binding.get('context_groups')
    if groups is not None:
        if [excerpt for group in groups for excerpt in group['source_excerpts']] != context:
            raise ArtifactIntegrityError('source context groups changed their bound selections')
        for group in groups:
            source_excerpts_text(artifacts, group['source_excerpts'],
                                 source_join=group['source_join'], repository=repository)
    return text, excerpts


def bind_source_body(artifacts, snapshot, target, parts=None, *, source_projection_ref=None, context_groups=None,
                     repository=None, cache=None):
    """Create a new target value only after exact host materialization succeeds."""
    parts = parts if parts is not None else [
        {'source_excerpt': excerpt} for excerpt in target.get('source_excerpts', [])]
    binding = BodyBinding.model_validate({'contract': SOURCE_BODY_CONTRACT,
        'authoring_input': snapshot, 'target_id': target['target_id'], 'kind': target['kind'],
        'subject': target.get('subject'), 'parts': parts,
        'context_source_excerpts': target.get('source_context_excerpts', []),
        'context_groups': context_groups,
        'source_projection_ref': source_projection_ref}).model_dump(exclude_none=True)
    text, excerpts = _materialize(artifacts, snapshot, binding, repository or EvidenceRepository(artifacts), cache)
    result = {**deepcopy(target), 'purpose': text, 'source_excerpts': excerpts,
              'source_body_contract': SOURCE_BODY_CONTRACT}
    result['source_body_ref'] = artifacts.put_json(binding)
    ResponseTarget.model_validate(result)
    return result


def load_source_body(artifacts, snapshot, target, *, repository=None, cache=None):
    """Reject changed text, selections or source versions before structure/Word."""
    try:
        value = ResponseTarget.model_validate(target).model_dump(exclude_none=True)
        if value.get('source_body_contract') != SOURCE_BODY_CONTRACT:
            raise ArtifactIntegrityError('target does not declare an exact source body')
        binding = BodyBinding.model_validate(artifacts.read_json(value['source_body_ref'])).model_dump(exclude_none=True)
        if (binding['authoring_input'] != _input(snapshot)
                or any(binding.get(key) != value.get(key) for key in ('target_id', 'kind', 'subject'))
                or binding['context_source_excerpts'] != value.get('source_context_excerpts', [])):
            raise ArtifactIntegrityError('body binding belongs to another input, target or context')
        text, excerpts = _materialize(artifacts, snapshot, binding, repository or EvidenceRepository(artifacts), cache)
        if text != value['purpose'] or excerpts != value['source_excerpts']:
            raise ArtifactIntegrityError('target body changed after its source binding')
        return text
    except (OSError, KeyError, TypeError, ValueError) as error:
        if isinstance(error, ArtifactIntegrityError):
            raise
        raise ArtifactIntegrityError('exact source body binding is unavailable or invalid') from error


def project_context_blocker(frozen):
    if any(issue.get('code') == 'source_roles_unconfirmed' for issue in frozen.get('issues', [])):
        return '请先核对项目文件的来源角色，再绑定方案背景。'
    if any(source.get('role') in (DocumentRole.AMENDMENT, DocumentRole.CLARIFICATION)
           for source in frozen.get('documents', [])):
        return '主件有补遗或澄清，请核对适用范围后再绑定方案背景。'
    return None


def load_source_context(artifacts, snapshot, target, *, repository=None):
    """Use the accepted joins without turning context into response body."""
    repository = repository or EvidenceRepository(artifacts)
    if target.get('project_context_excerpts'):
        try:
            value = ResponseTarget.model_validate(target).model_dump(exclude_unset=True)
        except ValueError as error:
            raise ArtifactIntegrityError('方案背景没有对应的可写响应位置。') from error
        documents = _documents(artifacts, snapshot, project_context=True)
        main = {identity: source for identity, source in documents.items() if source['role'] == DocumentRole.MAIN}
        excerpts = value['project_context_excerpts']
        index_refs = [source['source_index_ref'] for source in main.values()]
        if any(excerpt['source_ref']['source_index_ref'] not in index_refs for excerpt in excerpts):
            raise ArtifactIntegrityError('方案背景必须来自当前冻结主件。')
        intervals = {}
        for excerpt in excerpts:
            digest = excerpt['source_ref']['source_index_ref']['sha256']
            for selected in excerpt['selectors']:
                occupied = intervals.setdefault((digest, selected['source_range_id']), [])
                start, end = selected['start'], selected['end']
                if start == end or any(start < last and first < end for first, last in occupied):
                    raise ArtifactIntegrityError('方案背景范围必须非空且不得重叠。')
                occupied.append((start, end))
        return _native_sources(artifacts, excerpts, main, repository)
    load_source_body(artifacts, snapshot, target, repository=repository)
    binding = BodyBinding.model_validate(artifacts.read_json(target['source_body_ref'])).model_dump(exclude_none=True)
    groups = binding.get('context_groups')
    if groups is None:
        return source_excerpts_text(artifacts, target.get('source_context_excerpts', []), repository=repository)
    return '\n'.join(source_excerpts_text(artifacts, group['source_excerpts'],
        source_join=group['source_join'], repository=repository) for group in groups)


def validate_outline_source_bodies(artifacts, outline, *, repository=None):
    repository = repository or EvidenceRepository(artifacts)
    cache = {}

    def validate(target):
        if (target.get('source_body_contract') is not None or target.get('source_body_ref') is not None
                or target.get('source_context_excerpts')):
            load_source_body(artifacts, outline['authoring_input'], target, repository=repository, cache=cache)
        elif target.get('project_context_excerpts'):
            load_source_context(artifacts, outline['authoring_input'], target, repository=repository)

    def visit(chapters):
        for chapter in chapters:
            for target in chapter.get('targets', []):
                validate(target)
            visit(chapter.get('children', []))

    visit(outline.get('chapters', []))
    for target in outline.get('unplaced_targets', []):
        validate(target)
