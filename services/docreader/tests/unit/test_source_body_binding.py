"""Exact native/reader composition stays versioned and cannot bless loose purpose."""
from copy import deepcopy
import hashlib
from io import BytesIO

from docx import Document
import pytest

from docreader.integrations.artifacts import ArtifactIntegrityError
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.authoring_structure import _structure_payload
from docreader.tender.source_body import bind_source_body, load_source_body, save_source_projection
from test_native_body_context import source_case


def mixed_case(tmp_path):
    artifacts, outline, target = source_case(tmp_path)
    snapshot = outline['authoring_input']
    frozen = artifacts.read_json(snapshot['frozen_ref'])['documents'][0]
    line = '公章）'
    projection = save_source_projection(artifacts, snapshot, [
        {'document_id': frozen['document_id'], 'pages': ['前页原文', '续表原行\n' + line]}])
    parts = [{'source_excerpt': deepcopy(target['source_excerpts'][0])},
             {'layout_span': {'document_id': frozen['document_id'], 'page': 1, 'line': 1,
                              'start': 0, 'end': len(line), 'line_sha256': hashlib.sha256(line.encode()).hexdigest()},
              'join': 'continuation'}]
    target = bind_source_body(artifacts, snapshot, target, parts, source_projection_ref=projection)
    outline['chapters'][0]['targets'][0] = target
    return artifacts, outline, target


def test_unregistered_continuation_survives_native_body_structure_and_word(tmp_path):
    artifacts, outline, target = mixed_case(tmp_path)
    expected = '投标人应提交完整实施方案。公章）'
    assert load_source_body(artifacts, outline['authoring_input'], target) == expected
    payload, _ = _structure_payload(artifacts, outline, [], outline_phase=True)
    support = payload['chapters'][0]['targets'][0]['source_support_ids'][0]
    assert next(s['text'] for s in payload['supports'] if s['support_id'] == support) == expected
    result = export_authoring_document(artifacts, outline, [])
    assert expected in [p.text for p in Document(BytesIO(artifacts.read_bytes(result['docx_ref']))).paragraphs]
    assert target['manual_fill'] is True and target['support_origins'] == ['tender']


@pytest.mark.parametrize('corruption', ['missing_binding', 'binding_bytes', 'target_id', 'subject',
                                      'context', 'projection_input', 'layout_hash', 'layout_bounds'])
def test_bound_body_rejects_wrong_identity_reference_or_source_characters(tmp_path, corruption):
    artifacts, outline, target = mixed_case(tmp_path)
    snapshot = outline['authoring_input']
    if corruption == 'missing_binding':
        target.pop('source_body_ref')
    elif corruption == 'binding_bytes':
        target['source_body_ref'] = artifacts.put_json({})
    elif corruption == 'target_id':
        target['target_id'] += '-other'
    elif corruption == 'subject':
        target['subject'] = '另一主体'
    elif corruption == 'context':
        target['source_context_excerpts'] = []
    else:
        binding = artifacts.read_json(target['source_body_ref'])
        if corruption == 'projection_input':
            projection = artifacts.read_json(binding['source_projection_ref'])
            projection['authoring_input']['source_version'] += 1
            binding['source_projection_ref'] = artifacts.put_json(projection)
        elif corruption == 'layout_hash':
            binding['parts'][1]['layout_span']['line_sha256'] = '0' * 64
        else:
            binding['parts'][1]['layout_span']['end'] += 1
        target['source_body_ref'] = artifacts.put_json(binding)
    with pytest.raises(ArtifactIntegrityError):
        load_source_body(artifacts, snapshot, target)


def test_full_layout_only_body_keeps_a_real_signature_continuation(tmp_path):
    artifacts, outline, target = mixed_case(tmp_path)
    binding = artifacts.read_json(target['source_body_ref'])
    part = binding['parts'][1]
    part['join'] = 'paragraphs'
    target = bind_source_body(artifacts, outline['authoring_input'], target, [part],
                              source_projection_ref=binding['source_projection_ref'])
    assert target['source_excerpts'] == []
    assert target['purpose'] == '公章）'
    assert load_source_body(artifacts, outline['authoring_input'], target) == '公章）'


@pytest.mark.parametrize('bad', ['duplicate', 'missing_projection', 'both_source_kinds'])
def test_host_cannot_bind_ambiguous_or_duplicated_body_parts(tmp_path, bad):
    artifacts, outline, target = mixed_case(tmp_path)
    binding = artifacts.read_json(target['source_body_ref'])
    parts = binding['parts']
    projection = binding['source_projection_ref']
    if bad == 'duplicate':
        parts.append(deepcopy(parts[1]))
    elif bad == 'missing_projection':
        projection = None
    else:
        parts[0]['layout_span'] = deepcopy(parts[1]['layout_span'])
    with pytest.raises(ArtifactIntegrityError):
        bind_source_body(artifacts, outline['authoring_input'], target, parts,
                         source_projection_ref=projection)


@pytest.mark.parametrize('consumer', ['word', 'structure'])
def test_stripping_authority_fields_cannot_bypass_context_validation(tmp_path, consumer):
    artifacts, outline, target = source_case(tmp_path)
    target.pop('source_body_contract')
    target.pop('source_body_ref')
    target['purpose'] = '被篡改的正文'
    with pytest.raises(ArtifactIntegrityError):
        if consumer == 'word':
            export_authoring_document(artifacts, outline, [])
        else:
            _structure_payload(artifacts, outline, [], outline_phase=True)


def test_layout_continuation_cannot_reverse_source_order(tmp_path):
    artifacts, outline, target = mixed_case(tmp_path)
    binding = artifacts.read_json(target['source_body_ref'])
    last = deepcopy(binding['parts'][1])
    first = deepcopy(last)
    first['layout_span'].update(start=0, end=1)
    last['layout_span'].update(start=1, end=3)
    last['join'] = 'paragraphs'
    with pytest.raises(ArtifactIntegrityError):
        bind_source_body(artifacts, outline['authoring_input'], target, [last, first],
                         source_projection_ref=binding['source_projection_ref'])


@pytest.mark.parametrize('first_kind', ['native', 'layout'])
def test_same_page_mixed_continuation_requires_additional_source_proof(tmp_path, first_kind):
    artifacts, outline, target = mixed_case(tmp_path)
    binding = artifacts.read_json(target['source_body_ref'])
    parts = binding['parts']
    projection = artifacts.read_json(binding['source_projection_ref'])
    projection['documents'][0]['pages'][0] = projection['documents'][0]['pages'][1]
    parts[1]['layout_span']['page'] = 0
    if first_kind == 'layout':
        parts[0], parts[1] = parts[1], parts[0]
        parts[0]['join'], parts[1]['join'] = 'paragraphs', 'continuation'
    with pytest.raises(ArtifactIntegrityError):
        bind_source_body(artifacts, outline['authoring_input'], target, parts,
                         source_projection_ref=artifacts.put_json(projection))


def test_one_outline_reads_a_shared_source_projection_once(tmp_path, monkeypatch):
    from docreader.tender.source_body import validate_outline_source_bodies
    artifacts, outline, target = mixed_case(tmp_path)
    binding = artifacts.read_json(target['source_body_ref'])
    targets = []
    for i in range(8):
        other = {**deepcopy(target), 'target_id': f'target-{i}'}
        targets.append(bind_source_body(artifacts, outline['authoring_input'], other, binding['parts'],
                                        source_projection_ref=binding['source_projection_ref']))
    outline['chapters'][0]['targets'] = targets
    original, reads = artifacts.read_json, []

    def read(reference):
        if reference == binding['source_projection_ref']:
            reads.append(reference)
        return original(reference)

    monkeypatch.setattr(artifacts, 'read_json', read)
    validate_outline_source_bodies(artifacts, outline)
    assert len(reads) == 1


@pytest.mark.asyncio
async def test_tampered_body_cannot_publish_an_outline_or_reach_a_model(tmp_path):
    from docreader.tender.authoring_pipeline import generate_document
    artifacts, outline, target = source_case(tmp_path)
    target['purpose'] = '被篡改的正文'
    published = []

    async def no_calls(*args, **kwargs):
        pytest.fail('invalid source body reached a model request')

    async def on_outline(reference):
        published.append(reference)

    with pytest.raises(ArtifactIntegrityError):
        await generate_document(artifacts, outline['authoring_input'], None, no_calls,
                                outline=outline, outline_only=True, on_outline=on_outline)
    assert not published


def test_task_loader_validates_body_and_keeps_context_separate(tmp_path):
    from docreader.tender.authoring import _OutlineContext
    from docreader.tender.authoring_pipeline import _load_task, plan_writing_tasks
    from docreader.tender.schema import AuthoringBasisContent
    artifacts, outline, target = source_case(tmp_path)
    context = _OutlineContext(artifacts, outline['authoring_input'])
    task = plan_writing_tasks(outline)[0]
    basis = {'response': AuthoringBasisContent().model_dump()}
    payload, _, _ = _load_task(artifacts, context, task, basis, [])
    view = payload['targets'][0]
    supports = {s['support_id']: s['text'] for s in payload['supports']}
    assert [supports[s] for s in view['source_context_support_ids']] == ['建设方案评价', '5']
    target['purpose'] = '被篡改的正文'
    task = plan_writing_tasks(outline)[0]
    with pytest.raises(ArtifactIntegrityError):
        _load_task(artifacts, context, task, basis, [])


def test_native_part_uses_its_canonical_physical_range_page(tmp_path):
    from docreader.tender.evidence.references import EvidenceRepository, SourceIndex
    artifacts, outline, target = mixed_case(tmp_path)
    binding = artifacts.read_json(target['source_body_ref'])
    parts = binding['parts']
    old_ref = target['source_excerpts'][0]['source_ref']['source_index_ref']
    index = EvidenceRepository(artifacts).source_index(old_ref)
    raw = deepcopy(index.raw)
    raw['units'][0]['locator']['page_ordinal'] = 99
    raw['source_contract']['units'][0]['physical_locator']['page_ordinal'] = 0
    native = SourceIndex(index.document, raw)
    new_ref = artifacts.put_json(native.descriptor(artifacts.put_json(raw)))
    frozen = artifacts.read_json(outline['authoring_input']['frozen_ref'])
    frozen['documents'][0]['source_index_ref'] = new_ref
    outline['authoring_input']['frozen_ref'] = artifacts.put_json(frozen)
    for excerpt in [parts[0]['source_excerpt'], *target['source_context_excerpts']]:
        excerpt['source_ref']['source_index_ref'] = new_ref
    projection = save_source_projection(artifacts, outline['authoring_input'], [
        {'document_id': index.document['document_id'], 'pages': ['前页原文', '续表原行\n公章）']}])
    bound = bind_source_body(artifacts, outline['authoring_input'], target, parts,
                             source_projection_ref=projection)
    assert load_source_body(artifacts, outline['authoring_input'], bound).endswith('公章）')
