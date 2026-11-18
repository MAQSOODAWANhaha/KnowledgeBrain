"""Owner-reviewed source assertions retain their immutable evidence and legal exits."""
from copy import deepcopy
from hashlib import sha256

import pytest

from docreader.llm.request import ModelContractError
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.authoring_excerpts import whole_source_excerpts, source_excerpts_text
from docreader.tender.evidence.references import EvidenceRepository, selector, fragment
from docreader.tender.directory_compile import documents_from_frozen, compile_directory, outline_from_compiled
from docreader.tender.directory_sources import SourceRefs
from docreader.tender.schema import AuthoringInput
from test_authoring_templates import create_source
from test_directory_compile import item, node, plan


def reviewed():
    from docreader.tender import reviewed_source_facts
    return reviewed_source_facts


def point(page, line=0):
    return {'document_id': 'document', 'page': page, 'line': line}


def window(page, start=0, end=None):
    return {'document_id': 'document', 'page': page, 'start_line': start, 'end_line': start if end is None else end}


def fixture(tmp_path):
    artifacts, index_ref, index = create_source(tmp_path / 'artifacts', [
        {'text': '由资信册和方案册组成\n资信册\n方案册'},
        {'text': '财务资料\n投标人和制造商分别提交\n原式与签章'},
        {'text': '实施方案\n完整实施职责'},
        {'text': '应制定保密方案\n包括管理与应急'},
    ])
    frozen_ref = artifacts.put_json({'documents': [{**index.document, 'source_index_ref': index_ref, 'role': 'main'}]})
    snapshot = AuthoringInput.model_validate({'schema_version': 2, 'run_id': 'reviewed', 'source_version': 1,
        'domain_revision': 1, 'frozen_ref': frozen_ref}).model_dump()
    documents = documents_from_frozen(artifacts, artifacts.read_json(frozen_ref))
    evidence = artifacts.put_json({'owner_review': 'explicit review, not an old accepted model response'})
    def fact(kind, unit, **extra):
        excerpts = whole_source_excerpts(artifacts, {'source_index_ref': index_ref, 'unit_id': f'unit:{unit}'})
        text = '\n'.join(r['text'] for r in index.ranges if r['unit_id'] == f'unit:{unit}')
        return {'kind': kind, 'label': kind, 'source_excerpts': excerpts,
            'source_text_sha256': sha256(text.encode()).hexdigest(), 'review_evidence_ref': evidence,
            'enterprise_confirmed': False, 'supersedes': [], **extra}
    subjects = [{'source': window(1, 1), 'role': role} for role in ('投标人', '制造商')]
    facts = [fact('subject_layout', 1, heading=point(1), response_kind='form', subjects=subjects),
        fact('source_group', 0, heading=point(0, 1), basis_windows=[window(0, 0, 1)], member_headings=[point(1)], title='资信文件'),
        fact('independent_body_response', 3, source_windows=[window(3, 0, 1)], subjects=[], entry=None)]
    manifest = {'schema_version': 1, 'contract': 'reviewed_source_facts_v1', 'authoring_input': deepcopy(snapshot),
        'source_projection_digest': SourceRefs(documents).digest, 'facts': facts}
    raw_plan = plan([{**node('financial', 1, 0), 'heading': point(1), 'body_windows': [window(1, 0, 2)]},
        {**node('solution', 2, 0), 'heading': point(2), 'body_windows': [window(2, 0, 1)], 'response_kind': 'narrative'}],
        [item('financial'), item('solution')])
    # The shared fixture helpers use another document identity.
    raw_plan['scopes'] = [{'title': None, 'items': [item('financial'), item('solution')]}]
    compiled = compile_directory(raw_plan, documents)
    return artifacts, snapshot, documents, manifest, compiled, fact


def load(data):
    artifacts, snapshot, documents, manifest, *_ = data
    return reviewed().load_reviewed_facts(artifacts, snapshot, documents, artifacts.put_json(manifest))


def test_owner_facts_are_verified_with_host_ids_and_do_not_assert_enterprise_fulfillment(tmp_path):
    data = fixture(tmp_path)
    facts = load(data)
    assert len(facts) == 3 and len({f['fact_id'] for f in facts}) == 3
    assert facts[0]['fact_id'].startswith('reviewed-fact-')
    assert all(f['enterprise_confirmed'] is False for f in facts)
    assert facts[2]['_source_text'] == '应制定保密方案\n包括管理与应急'
    assert load(data) == facts


@pytest.mark.parametrize('bad', ['snapshot', 'projection', 'text_digest', 'source_index', 'version', 'evidence', 'unknown', 'enterprise', 'supplied_id', 'schema_bool', 'snapshot_type'])
def test_wrong_source_or_unread_evidence_cannot_be_reused(tmp_path, bad):
    data = fixture(tmp_path)
    artifacts, snapshot, documents, manifest, *_ = data
    if bad == 'schema_bool':
        manifest['schema_version'] = True
    elif bad == 'snapshot_type':
        manifest['authoring_input'] = None
    elif bad == 'snapshot':
        manifest['authoring_input']['run_id'] = 'other-run'
    elif bad == 'projection':
        manifest['source_projection_digest'] = '0' * 64
    elif bad == 'text_digest':
        manifest['facts'][0]['source_text_sha256'] = '0' * 64
    elif bad in {'source_index', 'version'}:
        ref = manifest['facts'][0]['source_excerpts'][0]['source_ref']
        descriptor = artifacts.read_json(ref['source_index_ref'])
        descriptor['document']['document_version'] = 'other-version'
        ref['source_index_ref'] = artifacts.put_json(descriptor)
    elif bad == 'evidence':
        ref = manifest['facts'][0]['review_evidence_ref']
        artifacts.path(ref).chmod(0o644)
        artifacts.path(ref).write_bytes(b'corrupt')
    elif bad == 'enterprise':
        manifest['facts'][0]['enterprise_confirmed'] = True
    elif bad == 'supplied_id':
        manifest['facts'][0]['fact_id'] = 'old-target'
    else:
        manifest['facts'][0]['accepted_target'] = 'old-model-response'
    with pytest.raises(ValueError):
        load(data)


def test_successor_cannot_drop_or_silently_replace_a_reviewed_fact(tmp_path):
    data = fixture(tmp_path)
    artifacts, _, _, manifest, *_ = data
    previous = artifacts.put_json(manifest)
    old_id = load(data)[0]['fact_id']
    manifest['facts'][0]['subjects'] = []
    with pytest.raises(ValueError, match='supersed'):
        reviewed().validate_fact_successor(artifacts, previous, artifacts.put_json(manifest))
    manifest['facts'][0]['supersedes'] = [old_id]
    with pytest.raises(ValueError, match='evidence'):
        reviewed().validate_fact_successor(artifacts, previous, artifacts.put_json(manifest))
    manifest['facts'][0]['review_evidence_ref'] = artifacts.put_json({'owner_review': 'corrected single whole-form instance'})
    reviewed().validate_fact_successor(artifacts, previous, artifacts.put_json(manifest))
    with pytest.raises(ValueError):
        reviewed().validate_fact_successor(artifacts, previous, None)


def test_reviewed_subjects_and_groups_change_only_the_reviewed_material_fields(tmp_path):
    data = fixture(tmp_path)
    _, snapshot, documents, _, compiled, _ = data
    before = deepcopy(compiled['chapters'])
    reviewed().apply_reviewed_material_facts(compiled, documents, load(data))
    financial, solution = compiled['chapters']
    assert [s['role'] for s in financial['subjects']] == ['投标人', '制造商']
    assert financial['group_display_title'] == '资信文件'
    for actual, original in zip(compiled['chapters'], before):
        for field, value in original.items():
            if field not in {'subjects', 'group', 'group_heading'}:
                assert actual[field] == value
    assert solution == before[1]
    assert compiled['_reviewed_source_fact_applications'][0]['state'] == 'applied'
    outline = outline_from_compiled(snapshot, compiled)
    assert {t['subject'] for t in outline['chapters'][0]['children'][0]['targets']} == {'投标人', '制造商'}


def test_missing_group_member_is_named_pending_and_existing_same_source_group_is_completed(tmp_path):
    data = fixture(tmp_path)
    _, _, documents, manifest, compiled, _ = data
    manifest['facts'][1]['member_headings'].append(point(2))
    facts = load(data)
    first = compiled['chapters'][0]
    first.update(group='资信册', group_heading=point(0, 1), group_display_title='资信文件')
    reviewed().apply_reviewed_material_facts(compiled, documents, facts)
    assert all(c['group_heading'] == point(0, 1) for c in compiled['chapters'])
    compiled['chapters'].pop()
    reviewed().apply_reviewed_material_facts(compiled, documents, facts)
    assert any(i['code'] == 'outline_directory_reviewed_fact_pending' and '资信文件' in i['detail'] for i in compiled['issues'])


def test_reviewed_body_disposition_requires_every_fact_once_and_one_compatible_owner(tmp_path):
    data = fixture(tmp_path)
    _, _, documents, _, compiled, _ = data
    facts = load(data)
    identity = facts[2]['fact_id']
    raw = {'items': [], 'requirement_adjudications': [], 'reviewed_body_dispositions': [
        {'fact_id': identity, 'scope': 'chapter', 'chapter_id': 'solution'}]}
    for bad in ('document', 'procedure', 'not_applicable', 'chapters', 'missing', 'duplicate', 'wrong_owner', 'unknown'):
        value = deepcopy(raw)
        if bad in ('document', 'procedure', 'not_applicable', 'chapters'):
            value['reviewed_body_dispositions'][0]['scope'] = bad
        elif bad == 'missing':
            value['reviewed_body_dispositions'] = []
        elif bad == 'duplicate':
            value['reviewed_body_dispositions'] *= 2
        elif bad == 'wrong_owner':
            value['reviewed_body_dispositions'][0]['chapter_id'] = 'unknown-owner'
        else:
            value['reviewed_body_dispositions'][0]['fact_id'] = 'old-target'
        with pytest.raises(ModelContractError):
            pipeline._accept_classification_plan(value, compiled, documents, reviewed_source_facts=facts)
    accepted = pipeline._accept_classification_plan(raw, compiled, documents, reviewed_source_facts=facts)
    reviewed().apply_reviewed_classification(compiled, documents, accepted, facts)
    outline = outline_from_compiled(data[1], compiled)
    reviewed().bind_reviewed_outline_facts(outline, compiled, facts)
    target = next(t for c in outline['chapters'] for t in c['targets'] if t['purpose'] == facts[2]['_source_text'])
    assert target['kind'] == 'narrative' and target['manual_fill'] and target['source_excerpts']


def test_native_layout_mismatch_uses_independent_fact_disposition_without_changing_legacy_case(tmp_path):
    data = fixture(tmp_path)
    _, snapshot, documents, manifest, compiled, _ = data
    manifest['facts'][2]['source_windows'] = [window(1, 1)]
    facts = load(data)
    before = deepcopy(compiled.get('_requirement_cases', []))
    raw = {'items': [], 'requirement_adjudications': [], 'reviewed_body_dispositions': [
        {'fact_id': facts[2]['fact_id'], 'scope': 'chapter', 'chapter_id': 'solution'}]}
    result = pipeline._accept_classification_plan(raw, compiled, documents, reviewed_source_facts=facts)
    reviewed().apply_reviewed_body_dispositions(compiled, documents, result['reviewed_body_dispositions'], facts)
    assert compiled.get('_requirement_cases', []) == before
    outline = outline_from_compiled(snapshot, compiled)
    reviewed().bind_reviewed_outline_facts(outline, compiled, facts)
    target = next(t for c in outline['chapters'] for t in c['targets'] if t['purpose'] == facts[2]['_source_text'])
    assert target['source_excerpts'] and target['manual_fill']


def test_complete_snapshot_and_explicit_false_are_required_and_correction_survives_later_generations(tmp_path):
    data = fixture(tmp_path)
    artifacts, _, _, manifest, *_ = data
    previous = artifacts.put_json(manifest)
    old_id = load(data)[0]['fact_id']
    manifest['facts'][0].update(subjects=[], supersedes=[old_id],
        review_evidence_ref=artifacts.put_json({'owner_review': 'corrected explicit single instance'}))
    corrected = artifacts.put_json(manifest)
    reviewed().validate_fact_successor(artifacts, previous, corrected)
    reviewed().validate_fact_successor(artifacts, corrected, corrected)
    manifest['facts'].append({**deepcopy(manifest['facts'][2]), 'label': 'another owner assertion'})
    reviewed().validate_fact_successor(artifacts, corrected, artifacts.put_json(manifest))
    manifest['facts'][0]['enterprise_confirmed'] = 0
    with pytest.raises(ValueError):
        load(data)
    manifest['facts'][0]['enterprise_confirmed'] = False
    manifest['authoring_input'].pop('extract_refs')
    with pytest.raises(ValueError, match='complete'):
        load(data)


def test_required_native_needs_the_complete_grid_not_one_reviewed_row(tmp_path):
    from test_structural_packing import grid
    artifacts, index_ref, index = create_source(tmp_path / 'grid', [{'grid': grid(rows=3)}])
    frozen = artifacts.put_json({'documents': [{**index.document, 'source_index_ref': index_ref, 'role': 'main'}]})
    snapshot = AuthoringInput.model_validate({'schema_version': 2, 'run_id': 'native', 'source_version': 1,
        'domain_revision': 1, 'frozen_ref': frozen}).model_dump()
    documents = documents_from_frozen(artifacts, artifacts.read_json(frozen))
    excerpts = whole_source_excerpts(artifacts, {'source_index_ref': index_ref, 'unit_id': 'unit:0'})
    fact = {'kind': 'required_native', 'label': 'native carrier', 'source_excerpts': excerpts,
        'source_text_sha256': sha256('\n'.join(r['text'] for r in index.ranges).encode()).hexdigest(),
        'review_evidence_ref': artifacts.put_json({'owner_review': 'all three native rows required'}),
        'enterprise_confirmed': False}
    manifest = {'schema_version': 1, 'contract': 'reviewed_source_facts_v1', 'authoring_input': snapshot,
        'source_projection_digest': SourceRefs(documents).digest, 'facts': [fact]}
    verified, = reviewed().load_reviewed_facts(artifacts, snapshot, documents, artifacts.put_json(manifest))
    assert verified['_table_id'].startswith('table-')
    compiled = compile_directory(plan([{**node('native', 0, 0), 'heading': point(0)}], [item('native')]), documents)
    outline = outline_from_compiled(snapshot, compiled)
    reviewed().bind_reviewed_outline_facts(outline, compiled, [verified])
    record, = compiled['_reviewed_source_fact_applications']
    assert record['state'] == 'native_carried' and record['final_chapter_ids'] and record['target_ids']
    for chapter in pipeline._flatten(outline['chapters']):
        for target in chapter['targets']:
            target['template_refs'] = []
    reviewed().bind_reviewed_outline_facts(outline, compiled, [verified])
    assert record['state'] == 'unresolved' and record['target_ids'] == []
    fact['source_excerpts'][0]['selectors'] = fact['source_excerpts'][0]['selectors'][:1]
    fact['source_text_sha256'] = sha256(index.ranges[0]['text'].encode()).hexdigest()
    with pytest.raises(ValueError, match='complete'):
        reviewed().load_reviewed_facts(artifacts, snapshot, documents, artifacts.put_json(manifest))


@pytest.mark.asyncio
@pytest.mark.parametrize('joint', [False, True])
async def test_generate_preflights_review_before_A_and_runs_M_C_to_single_source_bound_word_body(tmp_path, monkeypatch, joint):
    from io import BytesIO
    import json
    from zipfile import ZipFile
    from docx import Document
    from docreader.llm.budget import RequestBudget
    from docreader.llm.request import ModelConfigurationError
    from docreader.tender.authoring_export import export_authoring_document
    from docreader.tender.native_requirement_sources import CONTRACT, CLASSIFICATION_OPERATION
    from test_directory_compile import fixture_titles
    data = fixture(tmp_path)
    artifacts, snapshot, documents, manifest, compiled, _ = data
    observed, receipts = [], []
    raw_plan = plan([{**node('financial', 1, 0), 'heading': point(1), 'body_windows': [window(1, 0, 2)]},
        {**node('solution', 2, 0), 'heading': point(2), 'body_windows': [window(2, 0, 1)], 'response_kind': 'narrative'}],
        [item('financial'), item('solution')])
    async def plans(*args, **kwargs):
        observed.append('A')
        return [raw_plan], []
    async def execute(task, request, validate):
        payload = json.loads(request['messages'][1]['content'][0]['text'])
        observed.append((task, payload))
        if task == 'directory_material':
            assert payload['operation'] == 'select_source_closed_materials_v3'
            assert len(payload['reviewed_source_facts']) == 2
            assert not any('subjects' in f or 'member_headings' in f for f in payload['reviewed_source_facts'])
            return validate({'chapter_titles': fixture_titles(payload)})
        if task == 'directory_requirement':
            if not joint:
                return validate({'assignments': []})
            reviewed = set(payload['reviewed_body_refs'])
            atoms = {ref for block in payload['source_scope'] for ref, _ in block['lines']}
            assert reviewed and reviewed < atoms
            assert 'source_role=reviewed_body' in request['messages'][0]['content']
            return validate({'assignments': [], 'native_dispositions': [
                {'refs': sorted(reviewed), 'kind': 'already_carried'},
                {'refs': sorted(atoms - reviewed), 'kind': 'navigation'}]})
        assert task == 'directory_classification'
        assert payload['operation'] == (CLASSIFICATION_OPERATION if joint else 'classify_with_reviewed_source_facts_v1')
        if joint:
            assert '短 fN' in request['messages'][0]['content'] and '短 cN' in request['messages'][0]['content']
        assert payload['reviewed_source_facts'][0]['fact_id'] == 'f1'
        owner = next(c['id'] for c in payload['chapters'] if c['response_kind'] == 'narrative')
        return validate({'items': [], 'requirement_adjudications': [], 'reviewed_body_dispositions': [
            {'fact_id': 'f1', 'scope': 'chapter', 'chapter_id': owner}]})
    async def receipt(records):
        receipts.append(records)
    monkeypatch.setattr(pipeline, '_request_directory_plans', plans)
    budget = RequestBudget(model='synthetic', context_tokens=131072, output_reserve=32768)
    options = dict(outline_only=True, directory_documents=documents, source_carrier_contract='source_carriers_closed_v1',
        reviewed_source_facts_ref=artifacts.put_json(manifest), on_source_closure=receipt)
    if joint:
        options['native_requirement_contract'] = CONTRACT
    bad_manifest = deepcopy(manifest)
    bad_manifest['source_projection_digest'] = '0' * 64
    with pytest.raises(ValueError):
        await pipeline.generate_document(artifacts, snapshot, budget, execute, **{**options,
            'reviewed_source_facts_ref': artifacts.put_json(bad_manifest)})
    assert observed == []
    with pytest.raises(ModelConfigurationError):
        await pipeline.generate_document(artifacts, snapshot, budget, execute, **{**options, 'source_carrier_contract': None})
    assert observed == []
    ref = await pipeline.generate_document(artifacts, snapshot, budget, execute, **options)
    outline = artifacts.read_json(ref)
    target = next(t for c in pipeline._flatten(outline['chapters']) for t in c['targets']
        if t['purpose'] == '应制定保密方案\n包括管理与应急')
    from docreader.tender.authoring_excerpts import source_excerpt_key
    assert [source_excerpt_key(e) for e in target['source_excerpts']] == [source_excerpt_key(e) for e in manifest['facts'][2]['source_excerpts']]
    assert receipts == [[]]
    exported = export_authoring_document(artifacts, outline, [])
    document = Document(BytesIO(artifacts.read_bytes(exported['docx_ref'])))
    body = '\n'.join(p.text for p in document.paragraphs)
    assert body.count('应制定保密方案') == 1 and body.count('包括管理与应急') == 1
    with ZipFile(BytesIO(artifacts.read_bytes(exported['package_ref']))) as archive:
        guide = archive.read(json.loads(archive.read('manifest.json'))['submission_notes']['package_path']).decode()
        assert '应制定保密方案' in guide and '企业事实未确认' in guide


def test_reviewed_required_carrier_checks_exact_source_owner_without_moving_content():
    from test_directory_material_closure import carrier_case, material_reply
    from docreader.tender.directory_material_closure import candidate_carriers, apply_carrier_closure, native_parts
    from docreader.tender.directory_choices import apply_directory_adjudications
    compiled, documents = carrier_case()
    carriers = candidate_carriers(compiled, documents)
    apply_directory_adjudications(compiled, documents, material_reply(compiled, documents, 'alternative')['directory_adjudications'])
    table_id = carriers[0]['table_id']
    fact = {'kind': 'required_native', 'fact_id': 'owner-reviewed', 'label': 'service native table',
        '_source_text': 'native source text', '_table_id': table_id, 'carrier_heading': compiled['chapters'][0]['heading']}
    compiled['_reviewed_required_native_table_ids'] = [table_id]
    apply_carrier_closure(compiled, documents, [], carriers)
    reviewed().bind_reviewed_carrier_results(compiled, [fact])
    assert compiled['_source_carrier_closure'][0]['reviewed_owner_state'] == 'source_bound'
    # The physical source remains present in a different, valid root.
    other = deepcopy(compiled['chapters'][0])
    other.update(id='other-root', heading={'document_id': 'tender', 'page': 0, 'line': 4})
    compiled['chapters'].append(other)
    for field in ('template_parts', 'template_segments', 'material_segments'):
        compiled['chapters'][0].pop(field, None)
    before = deepcopy(compiled['chapters'])
    apply_carrier_closure(compiled, documents, [], carriers)
    reviewed().bind_reviewed_carrier_results(compiled, [fact])
    assert native_parts(other) and compiled['chapters'] == before
    assert compiled['_source_carrier_closure'][0]['state'] == 'native_carried'
    assert compiled['_source_carrier_closure'][0]['reviewed_owner_state'] == 'unresolved'
    assert any(i['code'] == 'outline_directory_reviewed_fact_pending' for i in compiled['issues'])


@pytest.mark.parametrize('extra_cell', [False, True])
def test_exact_native_body_binding_never_renders_an_unselected_grid_row(tmp_path, extra_cell):
    from io import BytesIO
    from docx import Document
    from docreader.tender.authoring_export import export_authoring_document
    from docreader.tender.evidence.references import selector, fragment
    body = '独立保密方案：应保留完整实施应答。'
    cells = [{'row': 0, 'column': 0, 'row_span': 1, 'col_span': 1, 'text': body}]
    if extra_cell:
        cells.append({'row': 0, 'column': 1, 'row_span': 1, 'col_span': 1, 'text': '评委专用计分：不得成为企业应答。'})
    artifacts, index_ref, index = create_source(tmp_path / 'native-body', [{'text': '实施方案\n完整实施职责'},
        {'grid': {'row_count': 1, 'column_count': 2 if extra_cell else 1, 'cells': cells}}])
    frozen = artifacts.put_json({'documents': [{**index.document, 'source_index_ref': index_ref, 'role': 'main'}]})
    snapshot = AuthoringInput.model_validate({'schema_version': 2, 'run_id': 'native-body', 'source_version': 1,
        'domain_revision': 1, 'frozen_ref': frozen}).model_dump()
    documents = documents_from_frozen(artifacts, artifacts.read_json(frozen))
    row = next(r for r in index.ranges if r['unit_id'] == 'unit:1')
    cell = next(c for c in row['cell_intervals'] if c['column'] == 0)
    excerpt = {'source_ref': {'source_index_ref': index_ref, 'unit_id': 'unit:1'},
        'selectors': [selector(fragment(row, cell['start'], cell['end']))]}
    fact = {'kind': 'independent_body_response', 'label': 'native confidentiality', 'source_excerpts': [excerpt],
        'source_text_sha256': sha256(body.encode()).hexdigest(), 'review_evidence_ref': artifacts.put_json({'owner_review': 'exact first cell'}),
        'enterprise_confirmed': False, 'source_windows': [window(1)], 'subjects': [], 'entry': None}
    manifest = {'schema_version': 1, 'contract': 'reviewed_source_facts_v1', 'authoring_input': snapshot,
        'source_projection_digest': SourceRefs(documents).digest, 'facts': [fact]}
    facts = reviewed().load_reviewed_facts(artifacts, snapshot, documents, artifacts.put_json(manifest))
    compiled = compile_directory(plan([{**node('solution', 0, 0), 'heading': point(0), 'response_kind': 'narrative',
        'body_windows': [window(0, 0, 1)]}], [item('solution')]), documents)
    decisions = [{'fact_id': facts[0]['fact_id'], 'scope': 'chapter', 'chapter_id': 'solution'}]
    reviewed().apply_reviewed_body_dispositions(compiled, documents, decisions, facts)
    outline = outline_from_compiled(snapshot, compiled)
    reviewed().bind_reviewed_outline_facts(outline, compiled, facts)
    target = next(t for t in outline['chapters'][0]['targets'] if t['purpose'] == body)
    assert not target['rule_refs'] and target['source_excerpts']
    exported = export_authoring_document(artifacts, outline, [])
    rendered = Document(BytesIO(artifacts.read_bytes(exported['docx_ref'])))
    assert not rendered.tables
    assert '\n'.join(p.text for p in rendered.paragraphs).count(body) == 1


def test_equal_unbound_text_does_not_infer_or_erase_a_different_source(tmp_path):
    data = fixture(tmp_path)
    _, snapshot, documents, _, compiled, _ = data
    facts = load(data)
    body_fact = facts[2]
    owner = next(c for c in compiled['chapters'] if c['id'] == 'solution')
    owner['requirements'] = [body_fact['_source_text'], body_fact['_source_text']]
    decisions = [{'fact_id': body_fact['fact_id'], 'scope': 'chapter', 'chapter_id': 'solution'}]
    reviewed().apply_reviewed_body_dispositions(compiled, documents, decisions, facts)
    outline = outline_from_compiled(snapshot, compiled)
    reviewed().bind_reviewed_outline_facts(outline, compiled, facts)
    targets = [t for c in outline['chapters'] for t in c['targets'] if t['purpose'] == body_fact['_source_text']]
    assert len(targets) == 3 and sum(bool(t.get('source_excerpts')) for t in targets) == 1


def test_exact_legacy_origin_retained_pending_or_explicitly_retired_without_207_layout_body(tmp_path):
    data = fixture(tmp_path)
    _, snapshot, documents, manifest, compiled, _ = data
    from docreader.tender.directory_compile import apply_requirements
    manifest['facts'][2]['source_windows'] = [window(1, 1)]
    facts = load(data)
    apply_requirements(compiled, documents, [{**window(1, 1), 'scope': 'unassigned', 'chapter_ids': []}], complete_basis=True)
    original_case = deepcopy(compiled['_requirement_cases'][0])
    original_issue = next(i for i in compiled['issues'] if i.get('requirement_case_id') == original_case['id'])
    shadow, origins = reviewed().reviewed_classification_view(compiled, facts)
    assert not pipeline.requirement_cases(shadow, documents)
    assert compiled['_requirement_cases'][0] == original_case
    raw = {'items': [], 'requirement_adjudications': [], 'reviewed_body_dispositions': [
        {'fact_id': facts[2]['fact_id'], 'scope': 'unassigned', 'chapter_id': None}]}
    pending = deepcopy(compiled)
    accepted = pipeline._accept_classification_plan(raw, pending, documents, reviewed_source_facts=facts)
    reviewed().apply_reviewed_classification(pending, documents, accepted, facts)
    assert original_issue in pending['issues']
    raw['reviewed_body_dispositions'][0].update(scope='chapter', chapter_id='solution')
    accepted = pipeline._accept_classification_plan(raw, compiled, documents, reviewed_source_facts=facts)
    reviewed().apply_reviewed_classification(compiled, documents, accepted, facts)
    assert original_issue not in compiled['issues'] and compiled['_requirement_cases'][0] == original_case
    outline = outline_from_compiled(snapshot, compiled)
    reviewed().bind_reviewed_outline_facts(outline, compiled, facts)
    targets = [t for c in outline['chapters'] for t in c['targets']]
    assert sum(t['purpose'] == facts[2]['_source_text'] for t in targets) == 1
    assert not any(t['purpose'] == original_case['source_text'] for t in targets)
    record = next(r for r in compiled['_reviewed_source_fact_applications'] if r['fact_id'] == facts[2]['fact_id'])
    assert record['origin_requirement_cases'] == [original_case]


@pytest.mark.parametrize('bad_context', ['outside', 'other_source'])
def test_body_context_is_verified_against_the_same_frozen_document(tmp_path, bad_context):
    data = fixture(tmp_path)
    artifacts, _, _, manifest, *_ = data
    context = deepcopy(manifest['facts'][0]['source_excerpts'])
    if bad_context == 'outside':
        context[0]['selectors'][0]['end'] += 10000
    else:
        other, other_ref, _ = create_source(tmp_path / 'foreign', [{'text': 'foreign source'}])
        context = whole_source_excerpts(other, {'source_index_ref': other_ref, 'unit_id': 'unit:0'})
    manifest['facts'][2]['context_source_excerpts'] = context
    with pytest.raises(ValueError):
        load(data)


def test_new_C_schema_is_additive_and_short_facts_are_request_local(tmp_path):
    from docreader.llm.contracts import output_schema, response_model, output_contract_spec
    from docreader.llm.budget import RequestBudget
    from docreader.tender.directory_compile import ClassificationPlan
    from docreader.tender.directory_material_ids import MaterialIds
    facts = load(fixture(tmp_path))
    payload = {'operation': reviewed().REVIEWED_CLASSIFICATION_OPERATION,
        'reviewed_source_facts': reviewed().reviewed_body_summary(facts), 'chapters': [{'id': 'solution'}]}
    ids = MaterialIds(payload)
    assert ids.payload['reviewed_source_facts'][0]['fact_id'] == 'f1'
    assert ids.restore_payload() == payload
    raw = {'reviewed_body_dispositions': [{'fact_id': 'f1', 'scope': 'chapter', 'chapter_id': 'c1'}]}
    assert ids.decode(raw)['reviewed_body_dispositions'][0]['fact_id'] == facts[2]['fact_id']
    assert response_model('directory_classification', payload) is reviewed().ReviewedClassificationPlan
    assert response_model('directory_classification', {}) is ClassificationPlan
    assert 'reviewed_body_dispositions' not in output_schema('directory_classification', {})['properties']
    assert 'ReviewedClassificationPlan' in output_contract_spec()
    budget = RequestBudget(model='synthetic')
    assert 'reviewed_body_dispositions' in pipeline._request(budget, 'directory_classification', ids.payload)['messages'][0]['content']
    assert 'reviewed_body_dispositions' not in pipeline._request(budget, 'directory_classification', {})['messages'][0]['content']


@pytest.mark.parametrize('owner_kind', ['form', 'attachment'])
def test_reviewed_native_body_can_use_form_canonical_owner_while_original_form_stays_intact(tmp_path, owner_kind):
    data = fixture(tmp_path)
    _, snapshot, documents, _, compiled, _ = data
    compiled['chapters'][0]['response_kind'] = owner_kind
    facts = load(data)
    before = deepcopy(compiled['chapters'])
    raw = {'items': [], 'requirement_adjudications': [], 'reviewed_body_dispositions': [
        {'fact_id': facts[2]['fact_id'], 'scope': 'chapter', 'chapter_id': 'financial'}]}
    accepted = pipeline._accept_classification_plan(raw, compiled, documents, reviewed_source_facts=facts)
    reviewed().apply_reviewed_classification(compiled, documents, accepted, facts)
    assert compiled['chapters'] == before
    outline = outline_from_compiled(snapshot, compiled)
    reviewed().bind_reviewed_outline_facts(outline, compiled, facts)
    chapter = next(c for c in outline['chapters'] if c['title'] == before[0]['title'])
    assert any(t['kind'] == owner_kind for t in chapter['targets'])
    assert sum(t['kind'] == 'narrative' and t['purpose'] == facts[2]['_source_text'] for t in chapter['targets']) == 1


@pytest.mark.parametrize('bad_owner', ['entry', 'subject', 'cover', 'content', 'quote'])
def test_form_canonical_owner_keeps_entry_subject_and_presentation_guards(tmp_path, bad_owner):
    data = fixture(tmp_path)
    _, _, documents, _, compiled, _ = data
    facts = load(data)
    owner = next(c for c in compiled['chapters'] if c['id'] == 'financial')
    if bad_owner == 'entry':
        owner['entry'] = point(1)
    elif bad_owner == 'subject':
        owner['subjects'] = [{'source': window(1, 1), 'role': '制造商'}]
    elif bad_owner == 'quote':
        owner['response_kind'] = 'quote'
    else:
        owner['presentation'] = bad_owner
    original = deepcopy(compiled)
    with pytest.raises(ModelContractError):
        pipeline._accept_classification_plan({'items': [], 'requirement_adjudications': [], 'reviewed_body_dispositions': [
            {'fact_id': facts[2]['fact_id'], 'scope': 'chapter', 'chapter_id': 'financial'}]},
            compiled, documents, reviewed_source_facts=facts)
    assert compiled == original


def split_body(data, mode):
    artifacts, _, _, manifest, *_ = data
    fact = deepcopy(manifest['facts'][-1])
    fact['label'] += ' reviewed separately'
    excerpt = fact['source_excerpts'][0]
    index = EvidenceRepository(artifacts).source_index(excerpt['source_ref']['source_index_ref'])
    base = index.by_id[excerpt['selectors'][0]['source_range_id']]
    a, z = base['start'], base['end']
    middle = (a + z) // 2
    spans = {'split': [(a, middle), (middle, z)], 'overlap': [(a, middle + 2), (middle, z)],
             'reverse': [(middle, z), (a, middle)], 'partial': [(a, middle)]}[mode]
    excerpt['selectors'] = [selector(fragment(base, start, end)) for start, end in spans]
    fact['source_text_sha256'] = sha256(source_excerpts_text(artifacts, fact['source_excerpts']).encode()).hexdigest()
    return fact


@pytest.mark.parametrize('representation', ['same', 'split', 'overlap', 'reverse'])
def test_equal_body_coverage_cannot_create_two_independent_facts(tmp_path, representation):
    data = fixture(tmp_path)
    manifest = data[3]
    fact = deepcopy(manifest['facts'][-1]) if representation == 'same' else split_body(data, representation)
    fact['label'] += ' another assertion'
    manifest['facts'].append(fact)
    with pytest.raises(ValueError, match='duplicate|same.*scope|unique.*body'):
        load(data)


def test_same_role_with_different_proof_window_cannot_create_another_instance(tmp_path):
    data = fixture(tmp_path)
    body = data[3]['facts'][-1]
    body['subjects'] = [{'source': window(1, 1), 'role': '投标人'}]
    fact = deepcopy(body)
    fact['label'] += ' widened role proof'
    fact['subjects'][0]['source'] = window(1, 1, 2)
    data[3]['facts'].append(fact)
    with pytest.raises(ValueError, match='duplicate|same.*scope|unique.*body'):
        load(data)


def test_one_body_fact_rejects_indistinguishable_repeated_role_instances(tmp_path):
    data = fixture(tmp_path)
    data[3]['facts'][-1]['subjects'] = [{'source': window(1, 1), 'role': '投标人'},
        {'source': window(1, 1, 2), 'role': '投标人'}]
    with pytest.raises(ValueError, match='role'):
        load(data)


@pytest.mark.parametrize('scope', ['entry', 'role', 'partial'])
def test_distinct_source_coverage_or_real_instance_scope_is_not_merged(tmp_path, scope):
    data = fixture(tmp_path)
    body = data[3]['facts'][-1]
    fact = split_body(data, 'partial') if scope == 'partial' else deepcopy(body)
    fact['label'] += ' another real scope'
    if scope == 'entry':
        fact['entry'] = point(2)
    elif scope == 'role':
        body['subjects'] = [{'source': window(1, 1), 'role': '投标人'}]
        fact['subjects'] = [{'source': window(1, 1), 'role': '制造商'}]
    data[3]['facts'].append(fact)
    assert len(load(data)) == 4
