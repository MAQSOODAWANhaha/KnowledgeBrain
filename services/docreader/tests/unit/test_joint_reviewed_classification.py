"""Joint C preserves reviewed fact authority and its legacy provenance."""
from copy import deepcopy

import pytest

from docreader.llm.request import ModelContractError
from docreader.tender.directory_compile import apply_requirements, outline_from_compiled
from docreader.tender.native_requirement_flow import JointClassification, classification_output_bound
from docreader.tender.native_requirement_sources import SourceLedger
from docreader.tender.reviewed_source_facts import bind_reviewed_outline_facts
from test_reviewed_source_facts import fixture, load, window


def test_joint_C_keeps_reviewed_body_and_uses_bounded_dispositions(tmp_path):
    data = fixture(tmp_path)
    artifacts, snapshot, documents, manifest, compiled, _ = data
    manifest['facts'][2]['source_windows'] = [window(1, 1)]
    fact, = (f for f in load(data) if f['kind'] == 'independent_body_response')
    apply_requirements(compiled, documents, [{**window(1, 1), 'scope': 'unassigned', 'chapter_ids': []}], complete_basis=True)
    origin, = compiled['_requirement_cases']
    ledger = SourceLedger(artifacts, artifacts.read_json(snapshot['frozen_ref'])['documents'], chapters=compiled['chapters'])
    joint = JointClassification(ledger, compiled, documents, reviewed_body_facts=[fact])
    c = next(alias for alias, owner in ledger.chapter_map.items() if owner == 'solution')
    assert joint.payload['requirement_cases'] == []
    supplied, = joint.payload['reviewed_source_facts']
    assert supplied['fact_id'] == 'f1'
    assert supplied['source_windows'][0]['document_id'] == 'd1'
    assert classification_output_bound(joint.payload)['reviewed_fact_count'] == 1

    before = deepcopy(compiled)
    with pytest.raises(ModelContractError):
        joint.accept({'reviewed_body_dispositions': []})
    with pytest.raises(ModelContractError):
        joint.accept({'reviewed_body_dispositions': [{'fact_id': fact['fact_id'], 'scope': 'chapter', 'chapter_id': c}]})
    with pytest.raises(ModelContractError):
        joint.accept({'reviewed_body_dispositions': [{'fact_id': 'f1', 'scope': 'chapter', 'chapter_id': c}] * 2})
    decision = joint.accept({'reviewed_body_dispositions': [{'fact_id': 'f1', 'scope': 'chapter', 'chapter_id': c}]})
    assert compiled == before

    pending = deepcopy(compiled)
    joint.fallback(pending)
    pending_record, = pending['_reviewed_source_fact_applications']
    assert pending_record['state'] == 'unresolved'
    assert pending_record['origin_requirement_cases'] == [origin]
    assert any(issue.get('requirement_case_id') == origin['id'] for issue in pending['issues'])

    joint.apply(compiled, decision)
    assert not any(issue.get('requirement_case_id') == origin['id'] for issue in compiled['issues'])
    assert compiled['_requirement_cases'] == [origin]
    outline = outline_from_compiled(snapshot, compiled)
    bind_reviewed_outline_facts(outline, compiled, [fact])
    record, = compiled['_reviewed_source_fact_applications']
    assert record['state'] == 'body_bound' and record['origin_requirement_cases'] == [origin]
    targets = [t for chapter in outline['chapters'] for t in chapter['targets'] if t['target_id'] in record['target_ids']]
    target, = targets
    assert target['source_excerpts'] == fact['source_excerpts'] and target['manual_fill']


def test_joint_C_preserves_distinct_facts_with_a_shared_original_excerpt(tmp_path):
    from hashlib import sha256

    from docreader.tender.authoring_excerpts import source_excerpts_text, whole_source_excerpts
    from test_reviewed_source_facts import point

    data = fixture(tmp_path)
    artifacts, snapshot, documents, manifest, compiled, _ = data
    source = artifacts.read_json(snapshot['frozen_ref'])['documents'][0]
    first = manifest['facts'][2]
    entry = point(2)
    next(chapter for chapter in compiled['chapters'] if chapter['id'] == 'solution')['entry'] = entry
    first['entry'] = entry
    second = deepcopy(first)
    second['source_windows'].append(window(2, 1))
    second['source_excerpts'].extend(whole_source_excerpts(
        artifacts, {'source_index_ref': source['source_index_ref'], 'unit_id': 'unit:2'}))
    second['source_text_sha256'] = sha256(source_excerpts_text(artifacts, second['source_excerpts']).encode()).hexdigest()
    manifest['facts'].append(second)
    facts = [f for f in load(data) if f['kind'] == 'independent_body_response']
    ledger = SourceLedger(artifacts, [source], chapters=compiled['chapters'])
    joint = JointClassification(ledger, compiled, documents, reviewed_body_facts=facts)
    owner = next(alias for alias, chapter in ledger.chapter_map.items() if chapter == 'solution')
    prepared = joint.accept({'reviewed_body_dispositions': [
        {'fact_id': f'f{i}', 'scope': 'chapter', 'chapter_id': owner} for i in (1, 2)]})
    joint.apply(compiled, prepared)
    outline = outline_from_compiled(snapshot, compiled)
    bind_reviewed_outline_facts(outline, compiled, facts)
    records = compiled['_reviewed_source_fact_applications']
    assert [r['state'] for r in records] == ['body_bound', 'body_bound']
    assert [r['fact_id'] for r in records] == [f['fact_id'] for f in facts]
    assert records[0]['target_ids'] != records[1]['target_ids']
    assert records[0]['source_excerpts'] == records[1]['source_excerpts'][:len(records[0]['source_excerpts'])]


def test_joint_C_preserves_partially_overlapping_legacy_case_without_copying_its_body(tmp_path):
    data = fixture(tmp_path)
    artifacts, snapshot, documents, manifest, compiled, _ = data
    manifest['facts'][2]['source_windows'] = [window(3, 0)]
    fact, = (f for f in load(data) if f['kind'] == 'independent_body_response')
    apply_requirements(compiled, documents, [{**window(3, 0, 1), 'scope': 'unassigned'}], complete_basis=True)
    source, = artifacts.read_json(snapshot['frozen_ref'])['documents']
    ledger = SourceLedger(artifacts, [source], chapters=compiled['chapters'])
    joint = JointClassification(ledger, compiled, documents, reviewed_body_facts=[fact])
    capsule, = joint.payload['requirement_cases']
    assert capsule['manual_only']
    owner = next(alias for alias, chapter in ledger.chapter_map.items() if chapter == 'solution')
    result = {'reviewed_body_dispositions': [{'fact_id': 'f1', 'scope': 'chapter', 'chapter_id': owner}],
              'requirement_adjudications': [{'item_ids': ['q1'], 'scope': 'chapter', 'chapter_ids': [owner]}]}
    with pytest.raises(ModelContractError) as rejected:
        joint.accept(result)
    assert 'partially overlaps' in rejected.value.errors[0]['detail']
    result['requirement_adjudications'][0].update(scope='unassigned', chapter_ids=[])
    original = deepcopy(compiled['_requirement_cases'])
    joint.apply(compiled, joint.accept(result))
    assert compiled['_requirement_cases'] == original
    assert any(i.get('requirement_case_id') == original[0]['id'] for i in compiled['issues'])


def test_scoped_joint_C_rejects_hidden_interior_and_preserves_failed_case(tmp_path):
    artifacts, snapshot, documents, _, compiled, _ = fixture(tmp_path)
    for page in (0, 2, 3):
        apply_requirements(compiled, documents, [{**window(page, 0), 'scope': 'unassigned'}], complete_basis=True)
    ledger = SourceLedger(artifacts, artifacts.read_json(snapshot['frozen_ref'])['documents'], chapters=compiled['chapters'])
    joint = JointClassification(ledger, compiled, documents)
    scope = joint.scope_payload(('q1', 'q2'))
    first, last = [page['lines'][0][0] for page in scope['documents'][0]['pages']]
    raw = {'basis_groups': [{'ranges': [[first, last]]}], 'requirement_adjudications': [
        {'item_ids': ['q1'], 'scope': 'not_applicable', 'basis_id': 1},
        {'item_ids': ['q2'], 'scope': 'unassigned'},
    ]}
    with pytest.raises(ModelContractError) as rejected:
        joint.accept_scope(scope, raw)
    assert 'outside the supplied reading' in rejected.value.errors[0]['detail']
    raw['basis_groups'][0]['ranges'] = [[first, first]]
    result = joint.accept_scope(scope, raw)
    pending = compiled['_requirement_cases'][-1]
    joint.apply(compiled, joint.merge_scopes([result]))
    assert any(issue.get('requirement_case_id') == pending['id'] for issue in compiled['issues'])
    assert len(compiled['requirement_decisions']) == 3
    assert compiled['requirement_decisions'][0]['scope'] == 'not_applicable'


async def test_joint_C_budget_fallback_sends_measured_scopes_and_retains_failed_sources(tmp_path, monkeypatch):
    import json

    from docreader.llm.request import ModelBudgetError
    from docreader.tender.authoring_pipeline import _request_joint_classification
    from docreader.tender import native_requirement_flow as flow
    from test_native_requirement_sources import budget

    artifacts, snapshot, documents, _, compiled, _ = fixture(tmp_path)
    for page in (0, 2):
        apply_requirements(compiled, documents, [{**window(page, 0), 'scope': 'unassigned'}], complete_basis=True)
    ledger = SourceLedger(artifacts, artifacts.read_json(snapshot['frozen_ref'])['documents'], chapters=compiled['chapters'])
    joint = JointClassification(ledger, compiled, documents)
    real_gate = flow.require_classification_budget
    admitted = []
    sent = []

    def gate(resolved_budget, request, payload):
        if 'classification_scope' not in payload or len(payload['requirement_cases']) > 1:
            raise ModelBudgetError('simulated cap')
        measured = real_gate(resolved_budget, request, payload)
        admitted.append(request)
        return measured

    async def execute(_, request, validate):
        assert request in admitted
        payload = json.loads(request['messages'][1]['content'][0]['text'])
        sent.append(payload)
        cases = payload['requirement_cases']
        if not cases:
            return validate({})
        if cases[0]['id'] == 'q2':
            raise ModelBudgetError('simulated late budget rejection')
        return validate({'requirement_adjudications': [{'item_ids': ['q1'], 'scope': 'procedure'}]})

    monkeypatch.setattr(flow, 'require_classification_budget', gate)
    original = deepcopy(compiled['_requirement_cases'])
    await _request_joint_classification(compiled, joint, budget(), execute, {})
    assert [payload['classification_scope'] for payload in sent] == ['document_v1', 'cases_v1', 'cases_v1']
    assert compiled['_native_classification_budget']['held_case_count'] == 1
    assert [record['scope'] for record in compiled['requirement_decisions']] == ['procedure', 'unassigned']
    unresolved, = [issue for issue in compiled['issues'] if issue.get('requirement_case_id')]
    assert unresolved['requirement_case_id'] == original[1]['id']
    assert unresolved['detail'] == original[1]['source_text']
    from io import BytesIO

    from docx import Document
    from docreader.tender.authoring_export import export_authoring_document

    outline = outline_from_compiled(snapshot, compiled)
    output = export_authoring_document(artifacts, outline, [])
    word = Document(BytesIO(artifacts.read_bytes(output['docx_ref'])))
    paragraphs = [paragraph.text for paragraph in word.paragraphs]
    assert original[1]['source_text'] in '\n'.join(paragraphs[paragraphs.index('适用性与来源待核'):])
