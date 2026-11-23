from copy import deepcopy
import json

import pytest
from pydantic import ValidationError

from docreader.integrations.artifacts import ArtifactStore, ArtifactIntegrityError
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError, ModelBudgetError
from docreader.tender.directory_compile import RequirementPlan, ClassificationPlan, apply_requirements
from docreader.tender.evidence.references import SourceIndex
from docreader.tender.native_requirement_sources import (
    SourceLedger, NativeRequirementPlan, NativeClassificationPlan, NativeCaseView,
    plan_native_packs, native_output_bound, require_native_budget,
)

def J(value):
    return json.dumps(value,ensure_ascii=False,sort_keys=True,separators=(',',':'))


def synthetic(tmp_path, *, first='Alpha ', second=' Beta  0.5 / 5', label='name ', tail='tag', version='1'):
    store=ArtifactStore(tmp_path/'artifacts')
    units=[]
    for page, texts in enumerate([['4',label,'5.00',first],['',tail,'',second]]):
        units.append({'unit_id':f'page:{page}:table:0','kind':'table_region','ordinal':page,'text':'',
          'locator':{'locator_kind':'page_table','page_ordinal':page,'table_ordinal':0,'left':0,'right':100,'top':100,'bottom':0},
          'grid':{'row_count':1,'column_count':4,'cells':[{'row':0,'column':c,'row_span':1,'col_span':1,'text':t} for c,t in enumerate(texts)]}})
    raw={'schema_version':2,'units':units,'images':{},'ocr':{},'source_contract':{'page_manifest':[],'units':[
      {'unit_id':u['unit_id'],'section_id':'s','parent_section_id':None,'heading_path':'requirements','physical_locator':u['locator']} for u in units]}}
    document={'document_id':'source-document-uuid','document_version':version,'original_ref':store.put_bytes(b'frozen '+version.encode())}
    ix=SourceIndex(document,raw)
    ref=store.put_json(ix.descriptor(store.put_json(raw)))
    frozen={**document,'source_index_ref':ref}
    ledger=SourceLedger(store,[frozen],chapters=[{'id':'source-node-real-id-A','title':'Technical'}, {'id':'source-node-real-id-B','title':'Certificates'}])
    return ledger,store,ix,frozen


def refs(ledger, page, column):
    return [i for i,a in ledger.atoms.items() if a['page']==page and a.get('column')==column]


def reply(ledger, view=None, *, selected=None, scope='unassigned', chapter_ids=(), context=True):
    view=view or ledger.view()
    selected=selected or refs(ledger,0,3)+refs(ledger,1,3)
    selected=[r for r in selected if r in view.refs]
    ctx=[]
    groups=[]
    if context:
        score=[r for r in refs(ledger,0,2) if r in view.refs]
        names=[r for r in refs(ledger,0,1)+refs(ledger,1,1) if r in view.refs]
        if score:
            groups.append(
                {'refs': score, 'source_join': 'paragraphs', 'parent_id': None}
            )
            ctx += score
        if names:
            groups.append(
                {
                    'refs': names,
                    'source_join': 'continuation',
                    'parent_id': 1 if score else None,
                }
            )
            ctx += names
    rest=[r for r in view.refs if r not in selected+ctx]
    ds=[{'refs':selected,'kind':'requirement'}]
    if ctx:
        ds.append({'refs': ctx, 'kind': 'context'})
    if rest:
        ds.append({'refs': rest, 'kind': 'unresolved'})
    return {'assignments':[],'contexts':groups,'native_assignments':[{'body_refs':selected,'context_id':len(groups) or None,
      'source_join':'continuation','scope':scope,'chapter_ids':list(chapter_ids)}],'native_dispositions':ds}


def native_source(accepted):return accepted['native_records'][0]['source']


def test_new_models_extend_old_without_changing_legacy_schema():
    r = J(RequirementPlan.model_json_schema())
    c = J(ClassificationPlan.model_json_schema())
    assert issubclass(NativeRequirementPlan,RequirementPlan)
    assert issubclass(NativeClassificationPlan,ClassificationPlan)
    assert NativeRequirementPlan.model_validate({'assignments':[]}).assignments==[]
    assert J(RequirementPlan.model_json_schema())==r and J(ClassificationPlan.model_json_schema())==c
    assert 'native_assignments' not in RequirementPlan.model_fields
    with pytest.raises(ValidationError):
        RequirementPlan.model_validate({'native_assignments': []})


@pytest.mark.parametrize(('first','second'), [('Alpha ',' Beta  0.5 / 5'),('接口（甲、乙','丙），每类0.25分，最多8分。'),('a  b  ',' c   d  42')])
def test_exact_body_context_and_original_spacing(tmp_path,first,second):
    ledger,_,index,_=synthetic(tmp_path,first=first,second=second)
    before = J(index.raw)
    accepted = ledger.view().accept(reply(ledger))
    source=native_source(accepted)
    assert source['body']['text']==first+second
    assert [g['text'] for g in source['contexts']]==['5.00','name tag']
    assert '5.00' not in source['body']['text']
    assert ledger.validate_source(source)==source and J(index.raw)==before
    assert accepted['complete'] is False
    assert accepted['cases'] and all(not c['enterprise_confirmed'] for c in accepted['cases'])


def test_native_does_not_depend_on_used_layout_lines(tmp_path):
    ledger,_,_,_=synthetic(tmp_path)
    documents=[{'document_id':'source-document-uuid','role':'main','pages':['source-owned line','next']}]
    compiled={'chapters':[{'id':'A','title':'A','lines':[{'document_id':'source-document-uuid','page':0,'line':0}]}],'issues':[]}
    apply_requirements(compiled,documents,[{'document_id':'source-document-uuid','page':0,'start_line':0,'end_line':0,'scope':'chapter','chapter_ids':['A']}])
    assert not compiled['chapters'][0].get('requirements')
    assert native_source(ledger.view().accept(reply(ledger)))['body']['text']=='Alpha  Beta  0.5 / 5'


@pytest.mark.parametrize('bad',['duplicate','unknown','context_as_body','unused_context','copy_text','unknown_chapter','cycle','carried_without_proof'])
def test_rejects_invalid_source_graph_without_mutation(tmp_path,bad):
    ledger, _, _, _ = synthetic(tmp_path)
    view = ledger.view()
    raw = reply(ledger)
    before = J(ledger.atoms)
    if bad == 'missing':
        raw['native_dispositions'].pop()
    if bad == 'duplicate':
        raw['native_dispositions'].append(deepcopy(raw['native_dispositions'][0]))
    if bad == 'unknown':
        raw['native_assignments'][0]['body_refs'] = [max(view.refs) + 1]
    if bad == 'context_as_body':
        raw['native_assignments'][0]['body_refs'] = refs(ledger, 0, 2)
    if bad == 'unused_context':
        raw['native_assignments'][0]['context_id'] = None
    if bad == 'copy_text':
        raw['native_assignments'][0]['source_text'] = 'known source must not be echoed'
    if bad == 'unknown_chapter':
        raw['native_assignments'][0].update(
            scope='chapter', chapter_ids=['source-node-real-id-A']
        )
    if bad == 'cycle':
        raw['contexts'][0]['parent_id'] = 2
    if bad == 'carried_without_proof':
        raw['native_dispositions'][-1]['kind'] = 'already_carried'
    with pytest.raises((ModelContractError, ValidationError)):
        view.accept(raw)
    assert J(ledger.atoms)==before


def test_source_reversal_and_tampering_rejected(tmp_path):
    ledger, _, _, _ = synthetic(tmp_path)
    source = native_source(ledger.view().accept(reply(ledger)))
    with pytest.raises((ModelContractError, ArtifactIntegrityError)):
        ledger.source(
            list(reversed(refs(ledger, 0, 3) + refs(ledger, 1, 3))),
            [],
            source_join='continuation',
        )
    for field in ['text','identity','source_ref']:
        bad=deepcopy(source)
        if field == 'text':
            bad['body']['text'] = 'changed'
        if field == 'identity':
            bad['source_id'] = 'native-source-' + ('0' * 64)
        if field == 'source_ref':
            bad['body']['source_excerpts'][0]['source_ref']['source_index_ref'][
                'sha256'
            ] = '0' * 64
        with pytest.raises((ModelContractError, ArtifactIntegrityError)):
            ledger.validate_source(bad)


def test_records_from_another_frozen_document_cannot_merge(tmp_path):
    ledger, _, _, _ = synthetic(tmp_path / 'one')
    other, _, _, _ = synthetic(tmp_path / 'two', version='2')
    first=ledger.case(native_source(ledger.view().accept(reply(ledger))),origin='one')
    second=other.case(native_source(other.view().accept(reply(other))),origin='two')
    with pytest.raises((ModelContractError, ArtifactIntegrityError)):
        NativeCaseView(ledger, [first, second])


def test_cross_pack_cases_require_explicit_C_and_preserve_original_ids(tmp_path):
    ledger,_,_,_=synthetic(tmp_path)
    one,two=ledger.view([0]),ledger.view([1])
    forced = one.accept(reply(ledger,one,scope='chapter',chapter_ids=['c1'],context=False))
    assert forced['cases'] and all(record['scope'] == 'unassigned' for record in forced['native_records'])
    a=one.accept(reply(ledger,one,context=False))['cases'][0]
    b=two.accept(reply(ledger,two,context=False))['cases'][0]
    a=ledger.case(a['source'],origin=a['origin'],candidate_chapter_ids=['source-node-real-id-A'])
    b=ledger.case(b['source'],origin=b['origin'],candidate_chapter_ids=['source-node-real-id-B'])
    # Candidate owners are provenance, not an implicit merge decision.
    c = NativeCaseView(ledger, [a, b])
    before = J([a, b])
    pending = c.accept({'items':[],'requirement_adjudications':[]})
    assert [decision['scope'] for decision in pending['native_decisions']] == ['unassigned', 'unassigned']
    raw={'items':[],'requirement_adjudications':[{'item_ids':['q1','q2'],'scope':'chapter','chapter_ids':['c2'],
       'basis_windows':[],'source_join':'continuation'}]}
    out=c.accept(raw)
    resolved=out['native_decisions'][0]
    assert resolved['source']['body']['text']=='Alpha  Beta  0.5 / 5'
    assert resolved['chapter_ids']==['source-node-real-id-B']
    assert resolved['item_ids']==[a['id'],b['id']]
    assert resolved['enterprise_confirmed'] is False and J([a,b])==before
    tampered = deepcopy(a)
    tampered['source']['body']['text'] = 'changed'
    with pytest.raises((ModelContractError, ArtifactIntegrityError)):
        NativeCaseView(ledger, [tampered, b])


def test_shared_context_pool_serializes_source_once_and_keeps_numeric_context(tmp_path):
    ledger,_,_,_=synthetic(tmp_path)
    raw = reply(ledger)
    body = raw['native_assignments'][0]
    first, second = body['body_refs']
    raw['native_assignments']=[{**body,'body_refs':[first]},{**body,'body_refs':[second]}]
    accepted=ledger.view().accept(raw)
    assert len(accepted['native_records'])==2
    assert len(raw['contexts'])==2
    assert sum(len(x['refs']) for x in raw['contexts'])==3
    assert all([g['text'] for g in r['source']['contexts']]==['5.00','name tag'] for r in accepted['native_records'])


@pytest.mark.parametrize('kind',['evaluator_procedure','reference','unresolved'])
def test_non_response_dispositions_have_honest_outlet(tmp_path,kind):
    ledger, _, _, _ = synthetic(tmp_path)
    view = ledger.view()
    accepted=view.accept({'assignments':[],'contexts':[],'native_assignments':[],
      'native_dispositions':[{'refs':list(view.refs),'kind':kind}]})
    assert not accepted['native_records']
    assert accepted['guidance'] or accepted['cases']
    assert all(not r['enterprise_confirmed'] for r in accepted['guidance']+accepted['cases'])
    if kind == 'unresolved':
        assert accepted['complete'] is False


def budget(context=131072):
    return RequestBudget(context_tokens=context,model='grok/grok-4.7@500k',tokenizer='o200k_base',tokenized_authoring=True,reasoning_effort='low')


def test_complete_request_schema_and_actual_K_are_counted(tmp_path):
    ledger, _, _, _ = synthetic(tmp_path)
    view = ledger.view()
    small=native_output_bound(view)
    more=SourceLedger(ledger.artifacts,ledger.documents,chapters=[{'id':f'real-{i}','title':f'Chapter {i}'} for i in range(40)]).view()
    assert native_output_bound(more)['token_upper_bound']>small['token_upper_bound']
    measured=require_native_budget(budget(),view,{'chapters':view.chapter_payload},'Read exact source; no enterprise facts.')
    assert measured['input_tokens']>0 and measured['worst_accepted_output_tokens']==small['token_upper_bound']
    assert measured['schema_sha256']
    with pytest.raises(ModelBudgetError):
        require_native_budget(
            budget(1024), view, {'chapters': view.chapter_payload}, 'x' * 5000
        )


def test_B_output_reserve_does_not_charge_body_atoms_twice(tmp_path):
    ledger, _, _, _ = synthetic(tmp_path, first='\n'.join(f'Clause {i}' for i in range(84)))
    ledger = SourceLedger(ledger.artifacts, ledger.documents,
                          chapters=[{'id': f'chapter-{i}', 'title': f'Chapter {i}'} for i in range(30)])
    view = ledger.view()
    chosen = list(view.refs)
    raw = {'assignments': [], 'contexts': [], 'native_assignments': [
        {'body_refs': [ref], 'context_id': None, 'source_join': 'continuation',
         'scope': 'chapters', 'chapter_ids': list(ledger.chapter_map)} for ref in chosen],
        'native_dispositions': [{'refs': [ref], 'kind': 'requirement'} for ref in chosen]}
    assert len(view.accept(raw)['native_records']) == len(chosen)
    measured = require_native_budget(budget(), view, {'chapters': view.chapter_payload}, 'Select exact sources.')
    bound = measured['output_shape']
    assert len(J(raw).encode('ascii')) <= bound['compact_json_byte_upper_bound']
    assert measured['worst_accepted_output_tokens'] == bound['compact_json_byte_upper_bound'] + 256
    assert measured['worst_accepted_output_tokens'] <= measured['output_reserve']


def test_B_compact_envelope_covers_shared_context_and_legacy_continuations(tmp_path):
    ledger, _, _, _ = synthetic(tmp_path)
    ledger = SourceLedger(ledger.artifacts, ledger.documents,
                          chapters=[{'id': f'chapter-{i}', 'title': f'Chapter {i}'} for i in range(12)])
    view = ledger.view()
    raw = reply(ledger, scope='chapters', chapter_ids=ledger.chapter_map)
    block = [{'document_id': 'd1', 'page': 200, 'lines': [
        {'line': line, 'text': 'Exact source', 'source_role': 'prose'} for line in (99, 100, 101, 102)]}]
    windows = [{'document_id': 'd1', 'page': 200, 'start_line': line, 'end_line': line}
               for line in (99, 100, 101, 102)]
    cap = native_output_bound(view, legacy_block=block)['compact_json_byte_upper_bound']
    for primary_count in range(1, len(windows) + 1):
        raw['assignments'] = [{**window, 'scope': 'chapters', 'chapter_ids': list(ledger.chapter_map),
                               'continuation_windows': windows[primary_count:] if index == primary_count - 1 else []}
                              for index, window in enumerate(windows[:primary_count])]
        assert len(view.accept(raw, legacy_block=block)['assignments']) == primary_count
        assert len(J(raw).encode('ascii')) <= cap


def test_pack_complete_ranges_and_keep_oversize_record_unfinished(tmp_path):
    ledger,_,_,_=synthetic(tmp_path,first='Alpha '*2500)
    result=plan_native_packs(ledger,budget(4096),lambda v:{'chapters':v.chapter_payload},'Select exact source.')
    assert result['pending_ranges']
    assert any(p['range_ordinals']==[0] and p['outlet']=='manual_source_review' for p in result['pending_ranges'])
    all_ordinals=[i for v in result['views'] for i in v.range_ordinals]+[i for p in result['pending_ranges'] for i in p['range_ordinals']]
    assert sorted(all_ordinals)==list(range(len(ledger.ranges)))
    assert len(all_ordinals)==len(set(all_ordinals))


def test_unresolved_dispositions_rebuild_sources_in_frozen_order(tmp_path):
    ledger, _, _, _ = synthetic(tmp_path)
    view = ledger.view()
    raw = reply(ledger, context=False)
    expected = view.accept(raw)
    unresolved = raw['native_dispositions'].pop()['refs']
    assert len(unresolved) >= 2
    for ref in reversed(unresolved):
        raw['native_dispositions'].append({'refs': [ref], 'kind': 'unresolved'})

    actual = view.accept(raw)

    assert actual['cases'] == expected['cases']
    assert actual['native_records'] == expected['native_records']
    assert not actual['complete']


def test_disposition_coverage_still_rejects_duplicate_refs(tmp_path):
    ledger, _, _, _ = synthetic(tmp_path)
    raw = reply(ledger, context=False)
    unresolved = raw['native_dispositions'][-1]['refs']
    raw['native_dispositions'].append({'refs': [unresolved[0]], 'kind': 'unresolved'})

    with pytest.raises(ModelContractError) as rejected:
        ledger.view().accept(raw)
    assert 'exactly one disposition' in rejected.value.errors[0]['detail']


def test_omitted_source_atom_stays_pending_without_another_model_round(tmp_path):
    ledger, _, _, _ = synthetic(tmp_path)
    raw = reply(ledger, context=False)
    omitted = raw['native_dispositions'].pop()['refs']
    before = J(ledger.atoms)
    accepted = ledger.view().accept(raw)
    unresolved = [ref for item in accepted['native_dispositions'] if item['kind'] == 'unresolved' for ref in item['refs']]
    assert set(omitted) <= set(unresolved)
    assert not accepted['complete'] and J(ledger.atoms) == before


def test_full_synthetic_inventory_keeps_83_atoms_explicitly_unresolved(tmp_path):
    first = 'Alpha\nBeta\nGamma\nDelta\nPrefix '
    second = 'Suffix\nTheta\nIota\n0.25 / 8'
    ledger, _, index, frozen = synthetic(tmp_path, first=first, second=second,
                                        label='Name A\nName B\nName C ', tail='Suffix')
    raw = deepcopy(index.raw)
    raw['units'][0]['grid']['cells'][0]['text'] = '\n'.join(f'unknown-{i}' for i in range(83))
    changed = SourceIndex(index.document, raw)
    frozen['source_index_ref'] = ledger.artifacts.put_json(changed.descriptor(ledger.artifacts.put_json(raw)))
    ledger = SourceLedger(ledger.artifacts, [frozen], chapters=[{'id': 'owner', 'title': 'Technical'}])
    before = J(changed.raw)
    accepted = ledger.view().accept(reply(ledger))
    unresolved = [r for d in accepted['native_dispositions'] if d['kind'] == 'unresolved' for r in d['refs']]
    assert len(ledger.atoms) == 97 and len(unresolved) == 83
    assert native_source(accepted)['body']['text'] == first + second
    assert not accepted['complete'] and all(not c['enterprise_confirmed'] for c in accepted['cases'])
    assert J(changed.raw) == before


def test_short_alias_mapping_is_bound_to_request_identity(tmp_path):
    ledger,_,_,_=synthetic(tmp_path)
    other=SourceLedger(ledger.artifacts,ledger.documents,chapters=[{'id':'different-real-id','title':'Technical'},
                                                                {'id':'source-node-real-id-B','title':'Certificates'}])
    assert ledger.view().payload['source_binding']!=other.view().payload['source_binding']


def test_C_single_case_keeps_its_existing_explicit_join(tmp_path):
    ledger,_,_,_=synthetic(tmp_path)
    source=native_source(ledger.view().accept(reply(ledger)))
    case=ledger.case(source,origin='test')
    view=NativeCaseView(ledger,[case])
    result=view.accept({'items':[],'requirement_adjudications':[{'item_ids':['q1'],'scope':'chapter','chapter_ids':['c1'],'basis_windows':[]}]})
    assert result['native_decisions'][0]['source']==source
    with pytest.raises(ModelContractError):
        view.accept(
            {
                'items': [],
                'requirement_adjudications': [
                    {
                        'item_ids': ['q1'],
                        'scope': 'chapter',
                        'chapter_ids': ['c1'],
                        'basis_windows': [],
                        'source_join': 'paragraphs',
                    }
                ],
            }
        )


def test_source_context_body_cannot_overlap_even_through_case_materializer(tmp_path):
    ledger,_,_,_=synthetic(tmp_path)
    body=refs(ledger,0,3)
    with pytest.raises(ModelContractError):
        ledger.source(body, [{'refs': body, 'source_join': 'paragraphs'}])


def test_legacy_case_cannot_be_reinterpreted_as_native(tmp_path):
    ledger,_,_,_=synthetic(tmp_path)
    legacy={'id':'requirement-legacy','source_windows':[{'document_id':'source-document-uuid','page':0,'start_line':0,'end_line':0}],
            'source_text':'Alpha '}
    before=J(legacy)
    with pytest.raises(ModelContractError):
        NativeCaseView(ledger, [legacy])
    assert J(legacy)==before


def test_new_B_restores_bounded_legacy_prose_for_existing_placement(tmp_path):
    ledger, _, _, _ = synthetic(tmp_path)
    view = ledger.view()
    raw = reply(ledger)
    block=[{'document_id':'d1','page':2,'lines':[{'line':0,'text':'Need a plan.','source_role':'prose'},{'line':1,'text':'Keep original source.','source_role':'prose'}]}]
    raw['assignments']=[{'document_id':'d1','page':2,'start_line':0,'end_line':1,'continuation_windows':[],
                         'scope':'chapter','chapter_ids':['c1']}]
    accepted=view.accept(raw,legacy_block=block)
    assert accepted['assignments'][0]['document_id']=='source-document-uuid'
    assert accepted['assignments'][0]['chapter_ids']==['source-node-real-id-A']
    assert native_source(accepted)['body']['text']=='Alpha  Beta  0.5 / 5'
    measured=require_native_budget(budget(),view,{'block':block,'allow_legacy_assignments':True,'chapters':view.chapter_payload},'Select exact sources.')
    assert measured['output_shape']['legacy_window_limit']==2
    wrong = deepcopy(raw)
    wrong['assignments'][0]['end_line'] = 10**100
    with pytest.raises(ModelContractError):
        view.accept(wrong, legacy_block=block)
    duplicate = deepcopy(raw)
    duplicate['assignments'] *= 2
    with pytest.raises(ModelContractError):
        view.accept(duplicate, legacy_block=block)


def test_boolean_ref_is_not_a_source_integer(tmp_path):
    ledger,_,_,_=synthetic(tmp_path)
    raw = reply(ledger)
    raw['native_dispositions'][-1]['refs'] = [True]
    with pytest.raises(ModelContractError):
        ledger.view().accept(raw)


def test_C_capsule_has_physical_locator_without_original_UUIDs(tmp_path):
    ledger,_,_,_=synthetic(tmp_path)
    case=ledger.case(native_source(ledger.view().accept(reply(ledger))),origin='physical-source')
    view=NativeCaseView(ledger,[case])
    capsule=view.payload['requirement_cases'][0]
    assert capsule['source_locations']
    assert [p['page'] for p in capsule['source_locations']]==[0,1]
    assert 'source-document-uuid' not in J(view.payload) and 'source-node-real-id' not in J(view.payload)


def test_output_contract_factory_uses_only_actual_wire_payload(tmp_path,monkeypatch):
    ledger, _, _, _ = synthetic(tmp_path)
    view = ledger.view()
    payload = {**view.payload, 'chapters': view.chapter_payload}
    before=J(payload)
    factory=getattr(__import__('docreader.tender.native_requirement_sources', fromlist=['native_requirement_model']),'native_requirement_model')
    monkeypatch.setattr(ledger.artifacts,'read_json',lambda _:pytest.fail('Schema factory performed source I/O'))
    model=factory(payload)
    assert model.model_json_schema()==view.response_model().model_json_schema()
    assert J(payload)==before


def test_selected_page_edge_is_not_misrepresented_as_original_file_edge(tmp_path):
    ledger,_,_,_=synthetic(tmp_path)
    selected=SourceLedger(ledger.artifacts,ledger.documents,chapters=[{'id':'source-node-real-id-A','title':'Technical'}],
                          pages={'source-document-uuid':[1]})
    view=selected.view()
    assert view.payload['open_before'] is True
    assert view.payload['open_after'] is False
    accepted = view.accept(reply(selected,view,scope='chapter',chapter_ids=['c1'],context=False))
    assert accepted['cases'] and all(record['scope'] == 'unassigned' for record in accepted['native_records'])


def with_prose(tmp_path, *, middle=False, selected_pages=None):
    ledger, store, index, frozen = synthetic(tmp_path)
    raw = deepcopy(index.raw)
    prose = {'unit_id': 'paragraph-context', 'kind': 'section', 'ordinal': 2,
             'text': 'Shared condition  0.25 / 8',
             'locator': {'locator_kind': 'document', 'section_ordinal': 2},
             'physical_locator': {'page_ordinal': 0}}
    if middle:
        prose['ordinal'] = 1
        prose['physical_locator']['page_ordinal'] = 1
        raw['units'][1]['ordinal'] = 2
        raw['units'][1]['locator']['page_ordinal'] = 2
        raw['units'] = [raw['units'][0], prose, raw['units'][1]]
    else:
        raw['units'].append(prose)
    raw['source_contract']['units'] = [
        {'unit_id': u['unit_id'], 'section_id': 's', 'parent_section_id': None,
         'heading_path': 'Synthetic requirements',
         'physical_locator': u.get('physical_locator') or u['locator']} for u in raw['units']]
    changed = SourceIndex(index.document, raw)
    frozen['source_index_ref'] = store.put_json(changed.descriptor(store.put_json(raw)))
    pages = None if selected_pages is None else {frozen['document_id']: selected_pages}
    return SourceLedger(store, [frozen], chapters=ledger.chapter_payload, pages=pages)


def test_native_assignment_body_is_grid_only_but_prose_context_is_preserved(tmp_path):
    ledger = with_prose(tmp_path)
    prose = [r for r, a in ledger.atoms.items() if a['column'] is None]
    raw = reply(ledger)
    raw['native_dispositions'][-1]['refs'] = [r for r in raw['native_dispositions'][-1]['refs'] if r not in prose]
    raw['native_dispositions'][1]['refs'] += prose
    raw['contexts'].append({'refs': prose, 'parent_id': 2, 'source_join': 'paragraphs'})
    raw['native_assignments'][0]['context_id'] = 3
    result = ledger.view().accept(raw)
    assert native_source(result)['contexts'][-1]['text'] == 'Shared condition  0.25 / 8'
    bad = deepcopy(raw)
    old_body = bad['native_assignments'][0]['body_refs']
    bad['native_assignments'][0]['body_refs'] = prose
    bad['native_assignments'][0]['context_id'] = 2
    bad['contexts'].pop()
    bad['native_dispositions'][0]['refs'] = prose
    bad['native_dispositions'][1]['refs'] = [r for r in bad['native_dispositions'][1]['refs'] if r not in prose]
    bad['native_dispositions'][-1]['refs'] += old_body
    with pytest.raises(ModelContractError):
        ledger.view().accept(bad)


def test_native_C_cannot_promote_prose_body_without_an_exclusive_mapping(tmp_path):
    ledger = with_prose(tmp_path)
    prose = [r for r, a in ledger.atoms.items() if a['column'] is None]
    case = ledger.case(ledger.source(prose, []), origin='unmapped-prose-source')
    view = NativeCaseView(ledger, [case])
    with pytest.raises(ModelContractError):
        view.accept({'items': [], 'requirement_adjudications': [{
            'item_ids': ['q1'], 'scope': 'chapter', 'chapter_ids': ['c1'], 'basis_windows': []}]})
    pending = view.accept({'items': [], 'requirement_adjudications': [{
        'item_ids': ['q1'], 'scope': 'unassigned', 'chapter_ids': [], 'basis_windows': []}]})
    assert pending['native_decisions'][0]['outlet'] == 'manual_source_review'


@pytest.mark.parametrize('line', [
    {'line': 0, 'text': 'Unmapped source'},
    {'line': 0, 'text': 'Known table source', 'source_role': 'grid'},
    {'line': 0, 'text': 'Contradictory certificate', 'source_role': 'prose', 'unit_id': 'page:0:table:0'},
])
def test_legacy_prose_requires_explicit_non_grid_source_domain(tmp_path, line):
    ledger, _, _, _ = synthetic(tmp_path)
    raw = reply(ledger)
    raw['assignments'] = [{'document_id': 'd1', 'page': 0, 'start_line': 0, 'end_line': 0,
                          'scope': 'chapter', 'chapter_ids': ['c1']}]
    with pytest.raises(ModelContractError):
        ledger.view().accept(raw, legacy_block=[{'document_id': 'd1', 'page': 0, 'lines': [line]}])


def test_one_bounded_B_request_can_include_multiple_explicit_source_components(tmp_path):
    ledger = with_prose(tmp_path, middle=True, selected_pages=[0, 2])
    result = plan_native_packs(ledger, budget(), lambda view: {'chapters': view.chapter_payload}, 'Select source components.')
    assert len(result['views']) == 1 and not result['pending_ranges']
    view = result['views'][0]
    assert len(view.payload['source_components']) == 2
    assert view.payload['component_gaps'][0]['reason'] == 'unseen_source_ranges'
    assert view.payload['source_closure'] == 'complete_ranges_with_explicit_components_v1'
    body = refs(ledger, 0, 3) + refs(ledger, 2, 3)
    raw = reply(ledger, view, selected=body, context=False)
    with pytest.raises(ModelContractError):
        view.accept(raw)


def test_new_operation_dispatch_and_derived_fingerprints_only(tmp_path):
    from docreader.llm.contracts import response_model, output_contract_spec
    ledger, _, _, _ = synthetic(tmp_path)
    payload = {**ledger.view().payload, 'chapters': ledger.chapter_payload}
    assert issubclass(response_model('directory_requirement', payload), NativeRequirementPlan)
    assert response_model('directory_requirement', {}) is RequirementPlan
    assert response_model('directory_classification', {'operation': 'classify_with_native_requirement_cases_v1'}) is NativeClassificationPlan
    assert response_model('directory_classification', {}) is ClassificationPlan
    fingerprints = output_contract_spec(['directory_requirement_native', 'directory_classification_native'])
    assert {'NativeRequirementPlan', 'NativeClassificationPlan'} <= fingerprints.keys()


def test_proven_literal_body_in_B_and_C_keeps_original_lines_and_bounds(tmp_path):
    ledger, store, index, frozen = synthetic(tmp_path)
    raw = deepcopy(index.raw)
    for offset, (page, value) in enumerate(((1, 'Literal # rule '), (2, 'ends with 0.25'), (2, 'separate row'))):
        locator = {'locator_kind': 'page', 'page_ordinal': page}
        unit_id = f'literal-test-{offset}'
        raw['units'].append({'unit_id': unit_id, 'ordinal': offset + 2, 'kind': 'section',
                             'locator': locator, 'text_basis': 'pdf_literal', 'text': value})
        raw['source_contract']['units'].append({'unit_id': unit_id, 'section_id': 'literal',
            'parent_section_id': None, 'heading_path': '', 'physical_locator': locator})
    index = SourceIndex(index.document, raw)
    frozen['source_index_ref'] = store.put_json(index.descriptor(store.put_json(raw)))
    ledger = SourceLedger(store, [frozen], chapters=[{'id': 'first-chapter', 'title': 'Technical'}])
    literal = [ref for ref, atom in ledger.atoms.items() if atom['base'].get('text_basis') == 'pdf_literal']
    grid = [ref for ref in ledger.atoms if ref not in literal]
    old_binding = ledger.binding
    keys = [(frozen['document_id'], f'literal-test-{i}') for i in range(3)]
    locations = {keys[0]: (1, 4), keys[1]: (2, 2), keys[2]: (2, 4)}
    ledger.bind_literal_locations(locations, continuation_edges=[(keys[0], keys[1]), (keys[1], keys[2])])
    view = ledger.view()
    assert view.payload['source_domain'] == 'grid_and_certified_literal_body_v1'
    assert ledger.binding != old_binding
    assert native_output_bound(view)['literal_body_atoms'] == 3
    assert native_output_bound(view)['chapter_choice_occurrences'] == len(grid) + 3
    assert [row['literal_body_line'] for row in view.payload['source_scope'] if 'literal_body_line' in row] == [4, 2, 4]
    assert [row['literal_predecessor_ref'] for row in view.payload['source_scope']
            if 'literal_predecessor_ref' in row] == literal[:2]
    assert ledger.require_body(literal[1:]) is None
    raw = reply(ledger, view, selected=literal[:2], context=False)
    model = view.response_model()
    accepted = view.accept(raw)
    case, = [c for c in accepted['cases'] if c['source']['body']['text'].startswith('Literal #')]
    assert case['source']['body']['text'] == 'Literal # rule ends with 0.25'
    c = NativeCaseView(ledger, [case])
    assert c.payload['requirement_cases'][0]['source_locations'][1]['literal_predecessor_ref'] == literal[0]
    resolved = c.accept({'items': [], 'requirement_adjudications': [{'item_ids': ['q1'],
        'scope': 'chapter', 'chapter_ids': ['c1'], 'basis_windows': []}]})
    assert resolved['native_decisions'][0]['source'] == case['source']
    split = [ledger.case(ledger.source([ref], []), origin='boundary') for ref in literal[:2]]
    merged = NativeCaseView(ledger, split).accept({'items': [], 'requirement_adjudications': [{
        'item_ids': ['q1', 'q2'], 'scope': 'chapter', 'chapter_ids': ['c1'], 'basis_windows': [],
        'source_join': 'continuation'}]})
    assert merged['native_decisions'][0]['source'] == case['source']
    model.model_validate(raw)
    mixed = reply(ledger, view, selected=[grid[-1], literal[0]], context=False)
    with pytest.raises(ModelContractError):
        view.accept(mixed)
    with pytest.raises(ModelContractError):
        ledger.bind_literal_locations(locations, continuation_edges=[(keys[0], keys[2])])
    ledger.bind_literal_locations({keys[0]: (1, 4), keys[2]: (2, 4)})
    with pytest.raises(ModelContractError):
        view.accept(raw)
    gap = ledger.view()
    unproven = reply(ledger, gap, selected=literal[1:2], context=False)
    from pydantic import ValidationError
    with pytest.raises(ValidationError):
        gap.response_model().model_validate(unproven)
    with pytest.raises(ModelContractError) as rejected:
        gap.accept(unproven)
    assert rejected.value.errors[0]['path'] == ['native_assignments', 0, 'body_refs', 0]
    detail = rejected.value.errors[0]['detail']
    assert f'Ref {literal[1]}' in detail
    assert 'unresolved only in native_dispositions' in detail
    unproven['native_assignments'].clear()
    for disposition in unproven['native_dispositions']:
        if disposition['refs'] == literal[1:2]:
            disposition['kind'] = 'unresolved'
    assert any(case['source']['body']['text'] == 'ends with 0.25' for case in gap.accept(unproven)['cases'])
    with pytest.raises(ModelContractError):
        NativeCaseView(ledger, [ledger.case(ledger.source(literal[1:2], []), origin='unknown')]).accept({
            'items': [], 'requirement_adjudications': [{'item_ids': ['q1'], 'scope': 'chapter',
                'chapter_ids': ['c1'], 'basis_windows': []}]})
    with pytest.raises(ModelContractError):
        gap.accept(reply(ledger, gap, selected=literal[::2], context=False))
    cases = [ledger.case(ledger.source([ref], []), origin='split') for ref in literal[::2]]
    with pytest.raises(ModelContractError):
        NativeCaseView(ledger, cases).accept({'items': [], 'requirement_adjudications': [{
            'item_ids': ['q1', 'q2'], 'scope': 'chapter', 'chapter_ids': ['c1'], 'basis_windows': [],
            'source_join': 'continuation'}]})


def test_unused_context_still_rejects_without_echo(tmp_path):
    ledger, _, _, _ = synthetic(tmp_path)
    raw = reply(ledger)
    raw['native_assignments'][0]['context_id'] = None
    with pytest.raises(ModelContractError) as rejected:
        ledger.view().accept(raw)
    assert rejected.value.errors[0]['path'][0] == 'contexts'
    assert rejected.value.feedback_fragments == []


def test_missing_assignment_and_open_edge_stay_pending_without_another_round(tmp_path):
    ledger, _, _, _ = synthetic(tmp_path)
    view = ledger.view()
    missing = reply(ledger)
    missing['native_assignments'].clear()
    missing['contexts'].clear()
    missing['native_dispositions'][1]['kind'] = 'unresolved'
    accepted = view.accept(missing)
    assert accepted['cases'] and not accepted['complete']
    needs_c = reply(ledger)
    needs_c['native_dispositions'][0]['kind'] = 'external_action'
    needs_c['native_assignments'][0].update(scope='chapter', chapter_ids=[next(iter(ledger.chapter_map))])
    accepted = view.accept(needs_c)
    assert accepted['cases'] and all(record['scope'] == 'unassigned' for record in accepted['native_records'])


@pytest.mark.parametrize(('position', 'edge'), [(0, 'open_after'), (1, 'open_before')])
def test_native_boundary_recovery_identifies_ref_and_preserves_pending_case(tmp_path, position, edge):
    ledger, _, _, _ = synthetic(tmp_path)
    view = ledger.view([position])
    raw = reply(ledger, view, scope='chapter', chapter_ids=['c1'], context=False)
    accepted = view.accept(raw)
    assert all(record['scope'] == 'unassigned' for record in accepted['native_records'])
    retained = next(case for case in accepted['cases'] if case['source']['body']['text'] in ('Alpha ', ' Beta  0.5 / 5'))
    assert retained['source']['body']['text'] == ('Alpha ', ' Beta  0.5 / 5')[position]
    assert not retained['enterprise_confirmed']
