"""A reviewed nonempty entry may resolve one host-known owner without inventing model output."""
from copy import deepcopy
from hashlib import sha256
from io import BytesIO
from zipfile import ZipFile
import json

from docx import Document
import pytest

from docreader.llm.request import ModelContractError
from docreader.tender import reviewed_source_facts as reviewed
from docreader.tender.authoring_excerpts import whole_source_excerpts, source_excerpts_text
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.directory_choices import _source_record
from docreader.tender.directory_compile import _Source, apply_requirements, compile_directory, documents_from_frozen, outline_from_compiled
from docreader.tender.directory_sources import SourceRefs
from docreader.tender.schema import AuthoringInput
from test_authoring_templates import create_source
from test_directory_compile import item, node, plan
from test_reviewed_source_facts import point, window


ENTRY_CONTRACT = 'reviewed_body_unique_entry_v1'


def source_case(tmp_path, *, entry=True, ambiguous=False):
    artifacts, index_ref, index = create_source(tmp_path / 'artifacts', [
        {'text': '投标文件组成\n实施方案\n备用方案\n保密范本仅供参考，本轮不作为提交正文'},
        {'text': '实施方案\n原有完整实施职责'},
        {'text': '应制定保密方案\n包括管理与应急'},
        {'text': '保密方案\n2分'},
        {'text': '备用方案\n备用实施职责'},
    ])
    frozen = artifacts.put_json({'documents': [{**index.document, 'source_index_ref': index_ref, 'role': 'main'}]})
    snapshot = AuthoringInput.model_validate({'schema_version': 2, 'run_id': 'unique-entry', 'source_version': 1,
        'domain_revision': 1, 'frozen_ref': frozen}).model_dump()
    documents = documents_from_frozen(artifacts, artifacts.read_json(frozen))
    excerpts = whole_source_excerpts(artifacts, {'source_index_ref': index_ref, 'unit_id': 'unit:2'})
    text = source_excerpts_text(artifacts, excerpts)
    fact = {'kind': 'independent_body_response', 'label': '已核保密正文', 'source_excerpts': excerpts,
        'context_source_excerpts': whole_source_excerpts(artifacts, {'source_index_ref': index_ref, 'unit_id': 'unit:3'}),
        'source_text_sha256': sha256(text.encode()).hexdigest(), 'review_evidence_ref': artifacts.put_json({'review': 'explicit source review'}),
        'enterprise_confirmed': False, 'subjects': [], 'entry': point(0, 1) if entry else None,
        'source_windows': [window(2, 0, 1)]}
    manifest = {'schema_version': 1, 'contract': 'reviewed_source_facts_v1', 'authoring_input': snapshot,
        'source_projection_digest': SourceRefs(documents).digest, 'facts': [fact]}
    reference = artifacts.put_json(manifest)
    facts = reviewed.load_reviewed_facts(artifacts, snapshot, documents, reference)
    nodes = [{**node('solution', 1, 0), 'heading': point(1), 'response_kind': 'narrative', 'body_windows': [window(1, 0, 1)]}]
    items = [item('solution', entry=0 if entry else None)]
    if ambiguous:
        nodes.append({**node('backup', 4, 0), 'heading': point(4), 'response_kind': 'narrative', 'body_windows': [window(4, 0, 1)]})
        items.append(item('backup', entry=0))
    compiled = compile_directory(plan(nodes, items, entries=[{'name': point(0, 1), 'excluded_by': []}] if entry else [],
        basis=[window(0, 0)] if entry else []), documents)
    if ambiguous:
        # Model/material revisions can expose two current owners of one valid entry.
        compiled['chapters'][1]['entry'] = point(0, 1)
        assert sum(reviewed._compatible_body(facts[0], c) for c in compiled['chapters']) == 2
    disposition = {'fact_id': facts[0]['fact_id'], 'scope': 'unassigned', 'chapter_id': None}
    return artifacts, snapshot, documents, manifest, reference, facts, compiled, disposition


@pytest.mark.parametrize('owner_kind', ['narrative', 'form', 'attachment'])
def test_unique_reviewed_entry_binds_without_rewriting_input_or_claiming_provider_choice(tmp_path, owner_kind):
    _, _, documents, _, _, facts, compiled, disposition = source_case(tmp_path)
    compiled['chapters'][0]['response_kind'] = owner_kind
    original = deepcopy(disposition)
    chapters = deepcopy(compiled['chapters'])
    reviewed.apply_reviewed_body_dispositions(compiled, documents, [disposition], facts, entry_contract=ENTRY_CONTRACT)
    record, = compiled['_reviewed_source_fact_applications']
    assert record['state'] == 'body_bound' and record['chapter_ids'] == ['solution']
    assert record['entry_contract'] == ENTRY_CONTRACT
    assert record['input_disposition'] == original and 'model_disposition' not in record
    assert record['host_resolution'] == {'kind': 'unique_entry', 'chapter_id': 'solution'}
    assert disposition == original and compiled['chapters'] == chapters


def test_valid_explicit_owner_is_not_relabelled_as_host_resolution(tmp_path):
    _, _, documents, _, _, facts, compiled, disposition = source_case(tmp_path)
    disposition.update(scope='chapter', chapter_id='solution')
    original = deepcopy(disposition)
    reviewed.apply_reviewed_body_dispositions(compiled, documents, [disposition], facts, entry_contract=ENTRY_CONTRACT)
    record, = compiled['_reviewed_source_fact_applications']
    assert record['state'] == 'body_bound' and record['chapter_ids'] == ['solution']
    assert record['input_disposition'] == original and 'host_resolution' not in record
    assert disposition == original


def test_classification_uses_effective_binding_to_retire_complete_legacy_origins(tmp_path):
    _, snapshot, documents, _, _, facts, compiled, disposition = source_case(tmp_path)
    apply_requirements(compiled, documents, [{**window(2, line), 'scope': 'unassigned'} for line in (0, 1)])
    original_cases = deepcopy(compiled['_requirement_cases'])
    value = {'items': [], 'requirement_adjudications': [], 'reviewed_body_dispositions': [disposition]}
    original = deepcopy(value)
    reviewed.apply_reviewed_classification(compiled, documents, value, facts, entry_contract=ENTRY_CONTRACT)
    assert not any(i['code'] == 'outline_directory_requirement_unassigned' for i in compiled['issues'])
    record, = compiled['_reviewed_source_fact_applications']
    assert record['state'] == 'body_bound' and record['origin_requirement_cases'] == original_cases
    assert compiled['_requirement_cases'] == original_cases and value == original
    outline = outline_from_compiled(snapshot, compiled)
    reviewed.bind_reviewed_outline_facts(outline, compiled, facts)
    reviewed.bind_reviewed_outline_facts(outline, compiled, facts)
    assert sum(t['purpose'] == facts[0]['_source_text'] for c in outline['chapters'] for t in c['targets']) == 1


@pytest.mark.parametrize('explicit_none', [False, True])
def test_old_default_unassigned_keeps_old_pending_behavior_and_record_fields(tmp_path, explicit_none):
    _, _, documents, _, _, facts, compiled, disposition = source_case(tmp_path)
    kwargs = {'entry_contract': None} if explicit_none else {}
    reviewed.apply_reviewed_body_dispositions(compiled, documents, [disposition], facts, **kwargs)
    record, = compiled['_reviewed_source_fact_applications']
    assert record['state'] == 'unresolved'
    assert not {'entry_contract', 'input_disposition', 'host_resolution', 'model_disposition'} & record.keys()
    assert any(i['code'] == 'outline_directory_reviewed_fact_pending' for i in compiled['issues'])


@pytest.mark.parametrize('mode', ['entry_none', 'no_owner', 'ambiguous'])
def test_no_entry_or_nonunique_owner_stays_unresolved_without_creating_a_chapter(tmp_path, mode):
    _, _, documents, _, _, facts, compiled, disposition = source_case(tmp_path,
        entry=mode != 'entry_none', ambiguous=mode == 'ambiguous')
    if mode == 'no_owner':
        compiled['chapters'] = []
    chapters = deepcopy(compiled['chapters'])
    reviewed.apply_reviewed_body_dispositions(compiled, documents, [disposition], facts, entry_contract=ENTRY_CONTRACT)
    record, = compiled['_reviewed_source_fact_applications']
    assert record['state'] == 'unresolved' and 'host_resolution' not in record
    assert compiled['chapters'] == chapters


@pytest.mark.parametrize('mode', ['entry', 'subject', 'cover', 'content', 'quote'])
def test_entry_resolution_does_not_relax_existing_owner_compatibility(tmp_path, mode):
    _, _, documents, _, _, facts, compiled, disposition = source_case(tmp_path)
    owner = compiled['chapters'][0]
    if mode == 'entry':
        owner['entry'] = point(0, 2)
    elif mode == 'subject':
        owner['subjects'] = [{'source': window(0, 1), 'role': '不同主体'}]
    elif mode in {'cover', 'content'}:
        owner['presentation'] = mode
    else:
        owner['response_kind'] = 'quote'
    before = deepcopy(compiled['chapters'])
    reviewed.apply_reviewed_body_dispositions(compiled, documents, [disposition], facts, entry_contract=ENTRY_CONTRACT)
    assert compiled['_reviewed_source_fact_applications'][0]['state'] == 'unresolved'
    assert compiled['chapters'] == before


@pytest.mark.parametrize('mode', ['wrong_owner', 'unknown_owner', 'duplicate', 'missing', 'unassigned_with_owner'])
def test_original_invalid_dispositions_are_rejected_before_any_host_resolution(tmp_path, mode):
    _, _, documents, _, _, facts, compiled, disposition = source_case(tmp_path)
    decisions = [deepcopy(disposition)]
    if mode == 'wrong_owner':
        decisions[0].update(scope='chapter', chapter_id='solution')
        compiled['chapters'][0]['entry'] = point(0, 2)
    elif mode == 'unknown_owner':
        decisions[0].update(scope='chapter', chapter_id='unknown-owner')
    elif mode == 'duplicate':
        decisions *= 2
    elif mode == 'missing':
        decisions = []
    else:
        decisions[0]['chapter_id'] = 'solution'
    original = deepcopy(compiled)
    with pytest.raises(ModelContractError):
        reviewed.apply_reviewed_body_dispositions(compiled, documents, decisions, facts, entry_contract=ENTRY_CONTRACT)
    assert compiled == original


def test_unique_entry_cannot_override_an_existing_same_source_exclusion(tmp_path):
    _, _, documents, _, _, facts, compiled, disposition = source_case(tmp_path)
    candidate = {'heading': point(2), 'lines': [point(2, line) for line in (0, 1)],
        'text': facts[0]['_source_text'], 'response_kind': 'narrative', 'subjects': [], 'presentation': 'section'}
    compiled['directory_decisions'] = [{'case_id': 'prior-source-exclusion', 'action': 'not_applicable_reference',
        'selected_candidate': candidate, 'source_records': [_source_record(candidate, _Source(documents), 'not_applicable_reference')],
        'basis_windows': [window(0, 3)], 'basis_text': documents[0]['pages'][0].splitlines()[3]}]
    before = deepcopy(compiled['directory_decisions'])
    reviewed.apply_reviewed_body_dispositions(compiled, documents, [disposition], facts, entry_contract=ENTRY_CONTRACT)
    record, = compiled['_reviewed_source_fact_applications']
    assert record['state'] == 'unresolved' and 'host_resolution' not in record
    assert 'not_applicable_reference' in record['entry_resolution_blocker']
    assert compiled['directory_decisions'] == before


def test_host_resolution_preserves_complete_native_body_context_word_and_guide(tmp_path):
    artifacts, snapshot, documents, _, reference, facts, compiled, disposition = source_case(tmp_path)
    original_facts = deepcopy(facts)
    summary = reviewed.reviewed_body_summary(facts)
    reviewed.apply_reviewed_body_dispositions(compiled, documents, [disposition], facts, entry_contract=ENTRY_CONTRACT)
    outline = outline_from_compiled(snapshot, compiled)
    reviewed.bind_reviewed_outline_facts(outline, compiled, facts)
    targets = [t for c in outline['chapters'] for t in c['targets'] if t['purpose'] == facts[0]['_source_text']]
    assert len(targets) == 1 and targets[0]['manual_fill'] and targets[0]['source_excerpts'] == facts[0]['source_excerpts']
    exported = export_authoring_document(artifacts, outline, [])
    word = Document(BytesIO(artifacts.read_bytes(exported['docx_ref'])))
    assert '\n'.join(p.text for p in word.paragraphs).count(facts[0]['_source_text']) == 1
    with ZipFile(BytesIO(artifacts.read_bytes(exported['package_ref']))) as package:
        manifest = json.loads(package.read('manifest.json'))
        guide = package.read(manifest['submission_notes']['package_path']).decode()
        assert facts[0]['_source_text'] in guide and facts[0]['_context_text'] in guide
    assert facts == original_facts and reviewed.reviewed_body_summary(facts) == summary
    assert reviewed.load_reviewed_facts(artifacts, snapshot, documents, reference) == original_facts


def test_unknown_entry_contract_is_rejected_without_mutation(tmp_path):
    _, _, documents, _, _, facts, compiled, disposition = source_case(tmp_path)
    before = deepcopy(compiled)
    with pytest.raises(ValueError, match='unsupported reviewed body entry contract'):
        reviewed.apply_reviewed_body_dispositions(compiled, documents, [disposition], facts, entry_contract='unknown')
    assert compiled == before
