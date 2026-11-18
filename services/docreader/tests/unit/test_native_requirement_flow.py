from copy import deepcopy
import json

import pytest

from docreader.llm.request import ModelContractError
from docreader.tender.directory_compile import compile_directory, apply_requirements, outline_from_compiled
from test_native_requirement_sources import synthetic, reply, native_source, refs


def test_prose_certificate_requires_exact_outside_grid_word_boxes():
    from docreader.tender.native_requirement_flow import certify_page
    grid={'unit_id':'grid','locator':{'left':10,'right':90,'top':80,'bottom':20}}
    physical={'height':100,'lines':[[[10,1,40,9,'Outside']],[[20,25,60,35,'Inside']],[[80,35,100,45,'Mixed']]]}
    result=certify_page(['Outside','Inside','Mixed','Unseen'],[None,None,None,None],[grid],physical)
    assert [x['source_role'] for x in result]==['prose','grid','unknown','unknown']
    assert result[1]['unit_id']=='grid'


def test_joint_C_uses_one_q_pool_and_atomic_existing_guards(tmp_path):
    from docreader.tender.native_requirement_flow import JointClassification
    ledger,_,_,_=synthetic(tmp_path)
    documents=[{'document_id':'source-document-uuid','role':'main','pages':['Technical\nLegacy instruction','continued'],
                'line_units':[[None,'page:0:table:0'],['page:1:table:0']]}]
    compiled={'chapters':[{'id':'source-node-real-id-A','title':'Technical','lines':[]}],'issues':[]}
    apply_requirements(compiled,documents,[{'document_id':'source-document-uuid','page':0,'start_line':1,'end_line':1,'scope':'unassigned'}])
    source=ledger.source(refs(ledger,0,3)+refs(ledger,1,3),[],source_join='continuation')
    compiled['_native_requirement_cases']=[ledger.case(source,origin='native')]
    joint=JointClassification(ledger,compiled,documents)
    assert len(joint.payload['requirement_cases'])==2
    assert [x['id'] for x in joint.payload['requirement_cases']]==['q1','q2']
    before=deepcopy(compiled)
    bad={'items':[],'basis_groups':[],'requirement_adjudications':[
      {'item_ids':['q1'],'scope':'unassigned','chapter_ids':[],'basis_id':None},
      {'item_ids':['q2'],'scope':'chapter','chapter_ids':['c1'],'basis_id':None,'source_join':'continuation'}]}
    prepared=joint.accept(bad)
    assert compiled==before
    joint.apply(compiled,prepared)
    assert compiled['_native_requirements'][0]['source']['body']['text']=='Alpha  Beta  0.5 / 5'
    assert compiled['_native_requirement_cases']==[]
    assert compiled['issues']


def test_native_cover_rejected_before_legacy_or_native_mutation(tmp_path):
    from docreader.tender.native_requirement_flow import JointClassification
    ledger,_,_,_=synthetic(tmp_path)
    docs=[{'document_id':'source-document-uuid','role':'main','pages':['Heading','Tail'],
           'line_units':[['page:0:table:0'],['page:1:table:0']]}]
    source=ledger.source(refs(ledger,0,3),[])
    compiled={'chapters':[{'id':'source-node-real-id-A','title':'Cover','presentation':'cover','lines':[]}],
              'issues':[],'_native_requirement_cases':[ledger.case(source,origin='x')]}
    before=deepcopy(compiled);joint=JointClassification(ledger,compiled,docs)
    with pytest.raises(ModelContractError):joint.accept({'items':[],'basis_groups':[],
      'requirement_adjudications':[{'item_ids':['q1'],'scope':'chapter','chapter_ids':['c1'],'basis_id':None}]})
    assert compiled==before


async def test_explicit_native_pipeline_uses_joint_B_and_one_C_then_binds_same_source(tmp_path):
    from docreader.tender.authoring_pipeline import _request_directory_details
    from docreader.tender.native_requirement_sources import CONTRACT, REQUIREMENT_OPERATION, CLASSIFICATION_OPERATION
    from test_native_requirement_sources import budget
    from test_directory_compile import node, item, plan
    ledger, store, _, frozen = synthetic(tmp_path)
    documents = [{'document_id': frozen['document_id'], 'role': 'main',
                  'pages': ['Technical\nMixed table layout', 'Continuation layout'],
                  'line_units': [[None, 'page:0:table:0'], ['page:1:table:0']]}]
    compiled = compile_directory(plan([node('technical', 0, 0, document_id=frozen['document_id'])], [item('technical')]), documents)
    identity = {'frozen_ref': store.put_json({'documents': [{**frozen, 'role': 'main'}]})}
    calls = []
    async def execute(task, request, validate):
        value = json.loads(request['messages'][1]['content'][0]['text']); calls.append((str(task), value))
        if task == 'directory_material':
            from test_directory_compile import fixture_titles
            return validate({'assignments': [], 'chapter_titles': fixture_titles(value)})
        if task == 'directory_requirement':
            assert value['operation'] == REQUIREMENT_OPERATION
            return validate(reply(ledger))
        assert task == 'directory_classification' and value['operation'] == CLASSIFICATION_OPERATION
        return validate({'items': [], 'basis_groups': [], 'requirement_adjudications': [
            {'item_ids': [c['id']], 'scope': 'chapter' if c.get('body') == 'Alpha  Beta  0.5 / 5' else 'unassigned',
             'chapter_ids': ['c1'] if c.get('body') == 'Alpha  Beta  0.5 / 5' else [], 'basis_id': None,
             'source_join': c.get('source_join')} for c in value['requirement_cases']]})
    await _request_directory_details(compiled, documents, budget(), execute, identity,
                                     artifacts=store, native_requirement_contract=CONTRACT)
    assert [t for t, _ in calls] == ['directory_material', 'directory_requirement', 'directory_classification']
    from test_authoring_pipeline_boundaries import snapshot as make_snapshot
    fixed = make_snapshot(store)
    outline = outline_from_compiled(fixed, compiled)
    targets = [t for chapter in outline['chapters'] for t in chapter['targets']]
    native = [t for t in targets if 'Alpha  Beta  0.5 / 5' in t['purpose']]
    assert len(native) == 1 and native[0]['manual_fill']
    assert not native[0]['source_refs'] and not native[0].get('source_excerpts') and not native[0].get('template_refs')
    guide = [i['detail'] for i in outline['issues'] if i['code'] == 'outline_directory_native_source_binding']
    assert len(guide) == 1 and native[0]['purpose'] in guide[0] and native[0]['target_id'] in guide[0]
    assert 'Mixed table layout' not in native[0]['purpose']


def test_C_output_bound_counts_all_case_chapter_edges_and_shared_basis():
    from docreader.tender.native_requirement_flow import classification_output_bound
    small = {'requirement_cases': [{'id': 'q1'}], 'chapters': [{'id': 'c1'}], 'source_line_count': 100, 'document_count': 1}
    large = {**small, 'requirement_cases': [{'id': f'q{i}'} for i in range(1, 101)],
             'chapters': [{'id': f'c{i}'} for i in range(1, 56)]}
    a, b = classification_output_bound(small), classification_output_bound(large)
    assert b['case_chapter_edges'] == 5500 and b['shared_basis_ranges'] == 100
    assert b['token_upper_bound'] > a['token_upper_bound'] and b['token_upper_bound'] > 32768


def test_native_output_bound_counts_only_grid_atoms_for_chapter_choices(tmp_path):
    from docreader.tender.native_requirement_sources import native_output_bound, SourceLedger
    ledger, store, index, frozen = synthetic(tmp_path)
    raw = deepcopy(index.raw)
    raw['units'].append({'unit_id': 'page:1:paragraph:9', 'kind': 'paragraph', 'ordinal': 2,
                         'text': 'Outside prose\nAnother line', 'locator': {'locator_kind': 'page', 'page_ordinal': 1}})
    raw['source_contract']['units'].append({'unit_id': 'page:1:paragraph:9', 'section_id': 's', 'parent_section_id': None, 'heading_path': 'requirements', 'physical_locator': raw['units'][-1]['locator']})
    from docreader.tender.evidence.references import SourceIndex
    fresh = SourceIndex({k: frozen[k] for k in ('document_id', 'document_version', 'original_ref')}, raw)
    frozen['source_index_ref'] = store.put_json(fresh.descriptor(store.put_json(raw)))
    mixed = SourceLedger(store, [frozen], chapters=ledger.chapter_payload)
    bound = native_output_bound(mixed.view())
    grid = sum(a['base'].get('text_basis') == 'grid_row' for a in mixed.atoms.values())
    assert len(mixed.atoms) > grid
    assert bound['chapter_choice_occurrences'] == grid * len(mixed.chapter_map)


async def test_no_contract_keeps_old_B_C_wire_and_schema(tmp_path):
    from docreader.tender.authoring_pipeline import _request_directory_details
    from test_directory_compile import node, item, plan, fixture_titles
    from test_native_requirement_sources import budget
    documents = [{'document_id': 'document', 'role': 'main', 'pages': ['Technical\nKnown prose']}]
    compiled = compile_directory(plan([node('technical', 0, 0, document_id='document')], [item('technical')]), documents)
    calls = []
    async def execute(task, request, validate):
        value = json.loads(request['messages'][1]['content'][0]['text']); calls.append((str(task), value))
        assert 'native_requirement_contract' not in value
        assert 'grid_body_verified_legacy_prose_v1' not in request['messages'][0]['content']
        if task == 'directory_material':
            return validate({'assignments': [], 'chapter_titles': fixture_titles(value)})
        return validate({'assignments': []} if task == 'directory_requirement' else {'items': []})
    await _request_directory_details(compiled, documents, budget(), execute)
    assert [t for t, _ in calls] == ['directory_material', 'directory_requirement', 'directory_classification']


def test_only_one_joint_pack_can_select_each_prose_line(tmp_path):
    from docreader.tender.native_requirement_flow import JointRequirements
    ledger, _, _, _ = synthetic(tmp_path)
    docs = [{'document_id': 'source-document-uuid', 'role': 'main',
             'pages': ['Outside\nGrid', 'Tail'], 'line_units': [[None, 'page:0:table:0'], ['page:1:table:0']]}]
    joint = JointRequirements(ledger, docs, {})
    joint.proof[('source-document-uuid', 0)][0]['source_role'] = 'prose'
    # Both complete ranges are placed on the same physical page in this host view.
    ledger.ranges[1]['wire']['page'] = 0
    one, two = joint.block(ledger.view([0])), joint.block(ledger.view([1]))
    assert one[0]['lines'][0]['source_role'] == 'prose'
    assert two[0]['lines'][0]['source_role'] == 'context'


def test_pdf_bbox_timeout_preserves_unknown_geometry_for_manual_review(tmp_path, monkeypatch):
    from subprocess import TimeoutExpired
    from docreader.tender import native_requirement_flow as flow
    ledger, _, _, _ = synthetic(tmp_path)
    documents = [{'document_id': 'source-document-uuid', 'role': 'main', 'pages': ['Unmapped'], 'line_units': [[None]]}]
    def timeout(*args):
        raise TimeoutExpired('pdftotext', 60)
    monkeypatch.setattr(flow, 'physical_pages', timeout)
    proof = flow.layout_certificates(ledger, documents)
    assert proof[('source-document-uuid', 0)][0]['source_role'] == 'unknown'


async def test_canonical_native_text_renders_once_in_Word_and_keeps_enterprise_blank(tmp_path):
    from io import BytesIO
    from docx import Document
    from docreader.tender.native_requirement_flow import bind_native_targets
    from docreader.tender.authoring_export import export_authoring_document
    from test_authoring_pipeline_boundaries import snapshot
    ledger, store, _, _ = synthetic(tmp_path)
    source = native_source(ledger.view().accept(reply(ledger)))
    compiled = {'_native_requirements': [{'source': source, 'scope': 'chapter', 'chapter_ids': ['source-node-real-id-A']} ]}
    chapter = {'chapter_id': 'chapter-1', 'title': 'Technical', 'targets': [], 'children': [], 'is_volume': False}
    issues = bind_native_targets(compiled, {'source-node-real-id-A': chapter})
    outline = {'schema_version': 2, 'authoring_input': snapshot(store), 'chapters': [chapter],
               'issues': [{**issue, 'rule_refs': []} for issue in issues]}
    from docreader.tender.authoring_pipeline import generate_document
    from test_native_requirement_sources import budget
    async def execute(task, request, validate):
        assert task != 'authoring_write'
        return validate({'shared_arrangements': []} if task == 'authoring_basis' else {'findings': []})
    result = store.read_json(await generate_document(store, outline['authoring_input'], budget(), execute, outline=outline))
    document = Document(BytesIO(store.read_bytes(result['docx_ref'])))
    text = '\n'.join(p.text for p in document.paragraphs).replace('\xa0', ' ')
    assert text.count('Alpha  Beta  0.5 / 5') == 1
    assert 'name tag' in text and '5.00' in text
    assert '________' in text
    assert not document.tables


@pytest.mark.asyncio
async def test_literal_body_guard_uses_only_its_line_and_withholds_unlocated(tmp_path):
    from docreader.tender.directory_compile import documents_from_frozen
    from docreader.tender.native_requirement_flow import source_windows
    from docreader.tender.native_requirement_sources import SourceLedger
    from test_pdf_literal_freeze import freeze_pdf

    artifacts, record, _, _ = await freeze_pdf(tmp_path, ['Exact source # 2.', 'Unrelated reference.'])
    documents = documents_from_frozen(artifacts, {'documents': [record]})
    ledger = SourceLedger(artifacts, [record])
    exact, unrelated = ledger.ranges
    ref = next(ref for ref, a in ledger.atoms.items() if a['range_ordinal'] == exact['ordinal'])
    source = ledger.source([ref], [])
    assert unrelated['base']['text'].strip() == 'Unrelated reference.'
    assert source_windows(ledger, source, documents) == []

    from docreader.llm.request import ModelContractError
    from docreader.tender.native_requirement_flow import JointClassification

    chapter = {'id': 'technical', 'title': 'Technical', 'lines': []}
    ledger = SourceLedger(artifacts, [record], chapters=[chapter])
    case = ledger.case(source, origin='manual:missing-layout')
    compiled = {'chapters': [chapter], 'issues': [], '_native_requirement_cases': [case]}
    classification = JointClassification(ledger, compiled, documents)
    assert classification.payload['requirement_cases'][0]['manual_only'] is True
    decision = {'item_ids': ['q1'], 'scope': 'chapter', 'chapter_ids': ['c1'], 'basis_id': None}
    with pytest.raises(ModelContractError) as rejected:
        classification.accept({'items': [], 'basis_groups': [], 'requirement_adjudications': [decision]})
    assert 'pending for manual review' in rejected.value.errors[0]['detail']
    decision.update(scope='unassigned', chapter_ids=[])
    prepared = classification.accept({'items': [], 'basis_groups': [], 'requirement_adjudications': [decision]})
    classification.apply(compiled, prepared)
    assert compiled['_native_requirement_cases'][0]['source'] == source

    doc = record['document_id']
    line = documents[0]['pages'][0].splitlines().index(exact['base']['text'])
    unit = exact['reference']['unit_id']
    proof = {(doc, 0): [{'line': i, 'unit_id': unit, 'source_role': 'native_literal'}
                       if i == line else {'line': i, 'source_role': 'prose'}
                       for i, _ in enumerate(documents[0]['pages'][0].splitlines())]}
    assert source_windows(ledger, source, documents, proof) == [
        {'document_id': doc, 'page': 0, 'start_line': line, 'end_line': line}]


def test_bound_source_word_preserves_source_spaces_and_empty_paragraphs():
    from docx import Document
    from docreader.tender.authoring_export import _write_carried_source
    from docreader.tender.schema import SOURCE_BODY_CONTRACT

    document = Document()
    assert _write_carried_source(document, {
        'source_body_contract': SOURCE_BODY_CONTRACT, 'purpose': ' Leading  gap\n\nTail. ',
    })
    assert [p.text for p in document.paragraphs] == [' Leading  gap', '', 'Tail. ']
    assert 'xml:space="preserve"' in document.paragraphs[0]._p.xml


def test_B_keeps_reviewed_grid_atoms_visible_but_blocks_body_and_layout_reselection(tmp_path):
    from docreader.tender.native_requirement_flow import JointRequirements
    from docreader.tender.native_requirement_sources import native_output_bound
    from test_native_requirement_sources import refs

    ledger, _, _, frozen = synthetic(tmp_path)
    doc = frozen['document_id']
    documents = [{'document_id': doc, 'role': 'main', 'pages': ['full\npartial', 'other'],
                  'line_units': [['page:0:table:0'] * 2, ['page:1:table:0']]}]
    full, = refs(ledger, 0, 1)
    partial, = refs(ledger, 0, 3)
    excerpts = ledger.excerpts([full]) + ledger.excerpts([partial])
    from docreader.tender.evidence.references import fragment, selector
    atom = ledger.atoms[partial]
    excerpts[-1]['selectors'][-1] = selector(fragment(atom['base'], atom['start'], atom['end'] - 1))
    facts = [{'kind': 'independent_body_response', 'source_excerpts': excerpts,
              'source_windows': [{'document_id': doc, 'page': 0, 'start_line': 0, 'end_line': 0}]}]
    baseline = native_output_bound(ledger.view())['grid_body_atoms']
    joint = JointRequirements(ledger, documents, {}, reviewed_body_facts=facts)
    view = ledger.view()
    payload = joint.payload(view)
    assert payload['reviewed_body_refs'] == [full]
    assert payload['reviewed_partial_refs'] == [partial]
    assert {full, partial} <= set(view.refs)
    assert native_output_bound(view)['grid_body_atoms'] == baseline - 2
    assert joint.block(view)[0]['lines'][0]['source_role'] == 'reviewed_body'
    dispositions = [{'refs': [full], 'kind': 'already_carried'},
                    {'refs': [partial], 'kind': 'reference'},
                    {'refs': [r for r in view.refs if r not in {full, partial}], 'kind': 'navigation'}]
    response = {'assignments': [], 'native_dispositions': dispositions}
    compiled = {'chapters': [], 'issues': []}
    assert not joint.accept(view, response, compiled)['cases']
    joint.reviewed_partial_geometry(compiled)
    issue, = compiled['issues']
    assert '与已核定正文部分重叠' in issue['detail'] and 'source_index_ref' not in issue['detail']
    assert ledger.artifacts.read_json(issue['source_evidence_ref'])['origin'] == 'manual:reviewed_partial_atom'
    for selected in (full, partial):
        bad = deepcopy(response)
        bad['native_assignments'] = [{'body_refs': [selected], 'scope': 'unassigned', 'chapter_ids': []}]
        with pytest.raises(ModelContractError):
            joint.accept(view, bad, {'chapters': [], 'issues': []})


def test_B_failed_pack_preserves_legacy_only_page_for_word_and_manual_C(tmp_path):
    from io import BytesIO
    from docx import Document
    from docreader.tender.authoring_export import export_authoring_document
    from docreader.tender.native_requirement_flow import JointClassification, JointRequirements
    from test_authoring_pipeline_boundaries import snapshot
    from test_directory_compile import item, node, plan

    ledger, artifacts, _, frozen = synthetic(tmp_path)
    doc = frozen['document_id']
    documents = [{'document_id': doc, 'role': 'main', 'pages': ['Technical', '原格', '独立正文要求 # 2'],
                  'line_units': [[None], ['page:1:table:0'], [None]]}]
    chapter = node('technical', 0, 0, document_id=doc)
    chapter['body_windows'] = [{'document_id': doc, 'page': 0, 'start_line': 0, 'end_line': 0}]
    compiled = compile_directory(plan([chapter], [item('technical')]), documents)
    joint = JointRequirements(ledger, documents, {}, carried_points={(doc, 0, 0)})
    owner = joint.page_owner[doc, 2]
    joint.pending(compiled, [owner], 'joint_B_unfinished')
    joint.unknown_geometry(compiled)
    source, = [case for case in compiled.get('_requirement_cases', ()) if case['source_windows'][0]['page'] == 2]
    classification = JointClassification(ledger, compiled, documents, proof=joint.proof)
    capsule = next(case for case in classification.payload['requirement_cases'] if case['source_kind'] == 'legacy')
    assert capsule['manual_only']
    outline = outline_from_compiled(snapshot(artifacts), compiled)
    delivery = export_authoring_document(artifacts, outline, [])
    paragraphs = [p.text for p in Document(BytesIO(artifacts.read_bytes(delivery['docx_ref']))).paragraphs]
    assert source['source_text'] in '\n'.join(paragraphs[paragraphs.index('适用性与来源待核'):])
