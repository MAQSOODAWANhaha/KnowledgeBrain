"""M must account for native carriers discarded by a source choice."""

from copy import deepcopy

import pytest
from pydantic import ValidationError

from docreader.llm.budget import RequestBudget
from docreader.llm.contracts import output_contract_spec, output_schema, response_model
from docreader.llm.request import ModelContractError, ModelWorkUnitExhausted
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.directory_choices import directory_cases, apply_directory_adjudications
from docreader.tender.directory_compile import MaterialPlan, NativeMaterialPlan, compile_plans
from docreader.tender.directory_material_ids import MaterialIds
from docreader.tender.directory_source_groups import MaterialSourceGroupPlan, SOURCE_GROUP_OPERATION
from test_directory_compile import fixture_titles, item, node, plan, window


CONTRACT = 'source_carriers_closed_v1'


def carrier_case(ends=(1, 3)):
    unit = 'page:0:table:0'
    documents = [{'document_id': 'tender', 'role': 'main',
        'pages': ['方案\n编写提示\n项目 值\n参数 A\n采用来源依据'],
        'line_units': [[None, None, unit, unit, None]],
        'template_parts': {unit: {'source_ref': {'source_index_ref': {'sha256': 'a' * 64}, 'unit_id': unit}, 'selectors': []}},
        'grids': [{'unit_id': unit, 'page': 0, 'rows': [['项目', '值'], ['参数', 'A']]}]}]
    plans = [plan([{**node('service', 0, 0), 'response_kind': 'narrative',
                    'body_windows': [window(0, 1, end)]}], [item('service')]) for end in ends]
    return compile_plans(plans, documents), documents


def material_reply(compiled, documents, action='current'):
    payload = {'chapter_title_catalog': pipeline.title_catalog(compiled, documents)}
    return {'chapter_titles': fixture_titles(payload), 'directory_adjudications': [
        {'case_id': value['case_id'], 'action': action, 'basis_windows': [window(0, 4)]}
        for value in directory_cases(compiled, documents)]}


def closure():
    from docreader.tender import directory_material_closure
    return directory_material_closure


def test_old_M_accepts_silent_native_loss_but_new_M_rejects_without_mutation():
    compiled, documents = carrier_case()
    raw = material_reply(compiled, documents)
    original = deepcopy(compiled)
    pipeline._accept_material_plan(raw, [], compiled, documents)
    with pytest.raises(ModelContractError) as caught:
        pipeline._accept_material_plan(raw, [], compiled, documents, source_carrier_contract=CONTRACT)
    assert caught.value.errors[0]['code'] == 'directory_material_carrier_unhandled'
    assert 'alternative' in caught.value.errors[0]['detail'] and 'unresolved' in caught.value.errors[0]['detail']
    assert compiled == original


@pytest.mark.parametrize('repair', ['alternative', 'assignment', 'unresolved', 'exclude'])
def test_existing_material_repairs_or_explicit_source_bound_outlets_close_the_difference(repair):
    compiled, documents = carrier_case()
    raw = material_reply(compiled, documents, 'alternative' if repair == 'alternative' else 'current')
    carriers = closure().candidate_carriers(compiled, documents)
    assert len(carriers) == 1
    if repair == 'assignment':
        raw['assignments'] = [{**window(0, 2, 3), 'chapter_id': 'service'}]
    if repair in {'unresolved', 'exclude'}:
        raw['carrier_dispositions'] = [{'table_id': carriers[0]['table_id'], 'action': repair,
            **({'case_id': carriers[0]['origins'][0]['case_id']} if repair == 'exclude' else {})}]
    pipeline._accept_material_plan(raw, [], compiled, documents, source_carrier_contract=CONTRACT)


def test_missing_carrier_keeps_all_candidate_origins_when_explicitly_unresolved():
    compiled, documents = carrier_case()
    module = closure()
    carriers = module.candidate_carriers(compiled, documents)
    chosen = deepcopy(compiled)
    apply_directory_adjudications(chosen, documents, material_reply(compiled, documents)['directory_adjudications'])
    module.apply_carrier_closure(chosen, documents,
        [{'table_id': carriers[0]['table_id'], 'action': 'unresolved'}], carriers)
    record, = chosen['_source_carrier_closure']
    assert record['state'] == 'unresolved' and record['origins'] == carriers[0]['origins']
    assert record['semantic_confirmed'] is False
    assert any(issue['code'] == 'outline_directory_material_carrier_unresolved' for issue in chosen['issues'])


def test_physical_catalog_deduplicates_the_table_but_keeps_both_candidate_origins():
    compiled, documents = carrier_case((3, 4))
    carriers = closure().candidate_carriers(compiled, documents)
    carrier, = carriers
    assert [origin['option'] for origin in carrier['origins']] == ['current', 'alternative']
    assert all(origin['source_record']['original_text'] for origin in carrier['origins'])
    catalog = closure().source_carrier_catalog(carriers)
    assert all('source_record' not in origin for origin in catalog[0]['origins'])
    assert 'source_ref' not in catalog[0]
    identities = MaterialIds({'source_carrier_catalog': catalog, 'chapters': [{'id': 'service'}]})
    assert identities.restore_payload()['source_carrier_catalog'] == catalog
    assert identities.payload['source_carrier_catalog'][0]['table_id'] == 't1'
    raw = {'carrier_dispositions': [{'table_id': 't1', 'action': 'reference', 'chapter_id': 'c1', 'case_id': 'q1'}]}
    assert identities.decode(raw)['carrier_dispositions'][0]['table_id'] == carrier['table_id']


def test_new_schema_and_prompt_activate_only_for_the_new_operation():
    module = closure()
    for model in (MaterialPlan, NativeMaterialPlan, MaterialSourceGroupPlan):
        with pytest.raises(ValidationError):
            model.model_validate({'carrier_dispositions': []})
    assert response_model('directory_material', {'operation': module.SOURCE_CARRIER_OPERATION}) is module.MaterialClosurePlan
    for operation in (None, 'select_response_forms_v1', SOURCE_GROUP_OPERATION):
        assert 'carrier_dispositions' not in output_schema('directory_material', {'operation': operation})['properties']
    assert 'MaterialClosurePlan' in output_contract_spec()
    budget = RequestBudget(model='synthetic')
    legacy = pipeline._request(budget, 'directory_material', {'operation': SOURCE_GROUP_OPERATION})
    new = pipeline._request(budget, 'directory_material', {'operation': module.SOURCE_CARRIER_OPERATION})
    assert 'carrier_dispositions' not in legacy['messages'][0]['content']
    assert 'carrier_dispositions' in new['messages'][0]['content']
    assert '物理' in new['messages'][0]['content']


@pytest.mark.parametrize('bad', ['unknown', 'duplicate', 'unrelated_case', 'missing_basis', 'extra_chapter'])
def test_bad_dispositions_reject_atomically(bad):
    compiled, documents = carrier_case()
    module = closure()
    carriers = module.candidate_carriers(compiled, documents)
    apply_directory_adjudications(compiled, documents, material_reply(compiled, documents)['directory_adjudications'])
    value = {'table_id': carriers[0]['table_id'], 'action': 'exclude', 'case_id': carriers[0]['origins'][0]['case_id']}
    selected = [value]
    if bad == 'unknown':
        value['table_id'] = 'unknown'
    elif bad == 'duplicate':
        selected.append(deepcopy(value))
    elif bad == 'unrelated_case':
        value['case_id'] = 'unknown'
    elif bad == 'missing_basis':
        compiled['directory_decisions'][0]['basis_windows'] = []
    else:
        value['chapter_id'] = 'service'
    original = deepcopy(compiled)
    with pytest.raises(ModelContractError):
        module.apply_carrier_closure(compiled, documents, selected, carriers)
    assert compiled == original


@pytest.mark.parametrize('protection', ['parameter', 'compiled'])
def test_reviewed_required_native_cannot_be_retired_by_normal_exclusion(protection):
    compiled, documents = carrier_case()
    module = closure()
    carriers = module.candidate_carriers(compiled, documents)
    apply_directory_adjudications(compiled, documents, material_reply(compiled, documents)['directory_adjudications'])
    identity = carriers[0]['table_id']
    options = {'required_native_ids': [identity]} if protection == 'parameter' else {}
    if protection == 'compiled':
        compiled['_reviewed_required_native_table_ids'] = [identity]
    value = {'table_id': identity, 'action': 'exclude', 'case_id': carriers[0]['origins'][0]['case_id']}
    with pytest.raises(ModelContractError) as caught:
        module.apply_carrier_closure(compiled, documents, [value], carriers, **options)
    assert 'superseding' in caught.value.errors[0]['detail']
    module.apply_carrier_closure(compiled, documents, [{'table_id': identity, 'action': 'unresolved'}], carriers, **options)
    assert compiled['_source_carrier_closure'][0]['state'] == 'unresolved'


@pytest.mark.parametrize('incompatible', ['role', 'subjects', 'entry', 'text_only', 'unknown', 'cover'])
def test_reference_needs_an_actual_same_native_part_and_compatible_canonical_owner(incompatible):
    compiled, documents = carrier_case()
    module = closure()
    carriers = module.candidate_carriers(compiled, documents)
    apply_directory_adjudications(compiled, documents, material_reply(compiled, documents, 'alternative')['directory_adjudications'])
    value = {'table_id': carriers[0]['table_id'], 'action': 'reference', 'chapter_id': 'service'}
    chapter = compiled['chapters'][0]
    if incompatible == 'role':
        chapter['response_kind'] = 'attachment'
    elif incompatible == 'subjects':
        chapter['subjects'] = [{'role': 'manufacturer'}]
    elif incompatible == 'entry':
        carriers[0]['origins'][0]['signature']['entry'] = {'source': 'one'}
        chapter['entry'] = {'source': 'two'}
    elif incompatible == 'text_only':
        chapter.pop('template_parts', None)
        chapter.pop('template_segments', None)
    elif incompatible == 'unknown':
        value['chapter_id'] = 'unknown'
    else:
        chapter['presentation'] = 'cover'
    with pytest.raises(ModelContractError) as caught:
        module.apply_carrier_closure(compiled, documents, [value], carriers)
    assert 'native part' in caught.value.errors[0]['detail']


def test_valid_reference_and_physical_presence_keep_semantics_unconfirmed():
    compiled, documents = carrier_case()
    module = closure()
    carriers = module.candidate_carriers(compiled, documents)
    apply_directory_adjudications(compiled, documents, material_reply(compiled, documents, 'alternative')['directory_adjudications'])
    module.apply_carrier_closure(compiled, documents,
        [{'table_id': carriers[0]['table_id'], 'action': 'reference', 'chapter_id': 'service'}], carriers)
    record, = compiled['_source_carrier_closure']
    assert record['state'] == 'reference' and record['chapter_ids'] == ['service'] and not record['semantic_confirmed']
    # One physical source table does not prove two independent subject instances.
    carriers[0]['origins'][0]['signature']['subjects'] = [{'role': 'manufacturer'}]
    module.apply_carrier_closure(compiled, documents, [], carriers)
    assert compiled['_source_carrier_closure'][0]['state'] == 'native_carried'
    assert compiled['_source_carrier_closure'][0]['semantic_confirmed'] is False


async def test_new_contract_uses_one_M_request_and_applies_closure_after_all_material_choices(monkeypatch):
    compiled, documents = carrier_case()
    calls = []

    async def ask(_execute, _budget, task, payload, validate):
        calls.append((str(task), deepcopy(payload)))
        if task == 'directory_material':
            assert payload['operation'] == closure().SOURCE_CARRIER_OPERATION
            assert payload['source_carrier_catalog']
            return validate(material_reply(compiled, documents, 'alternative'))
        return validate({'assignments': []} if task == 'directory_requirement' else {'items': []})

    monkeypatch.setattr(pipeline, '_ask', ask)
    await pipeline._request_directory_details(compiled, documents, None, None, source_carrier_contract=CONTRACT)
    assert [task for task, _ in calls].count('directory_material') == 1
    assert compiled['_source_carrier_closure'][0]['state'] == 'native_carried'


async def test_exhausted_M_keeps_named_native_pending_without_another_model_round(monkeypatch):
    compiled, documents = carrier_case()
    calls = []

    async def ask(_execute, _budget, task, _payload, validate):
        calls.append(str(task))
        if task == 'directory_material':
            raise ModelWorkUnitExhausted(task)
        return validate({'assignments': []} if task == 'directory_requirement' else {'items': []})

    monkeypatch.setattr(pipeline, '_ask', ask)
    await pipeline._request_directory_details(compiled, documents, None, None, source_carrier_contract=CONTRACT)
    assert calls.count('directory_material') == 1
    record, = compiled['_source_carrier_closure']
    assert record['state'] == 'unresolved' and record['origins']
    assert any(issue['code'] == 'outline_directory_material_rejected' for issue in compiled['issues'])


@pytest.mark.parametrize('present', [False, True])
def test_reviewed_required_table_outside_candidate_catalog_is_explicitly_recorded(present):
    compiled, documents = carrier_case()
    module = closure()
    identity = module.candidate_carriers(compiled, documents)[0]['table_id']
    apply_directory_adjudications(compiled, documents,
        material_reply(compiled, documents, 'alternative' if present else 'current')['directory_adjudications'])
    compiled['_reviewed_required_native_table_ids'] = [identity]
    module.apply_carrier_closure(compiled, documents, [], [])
    record, = compiled['_source_carrier_closure']
    assert record['table_id'] == identity and record['origins'] == []
    assert record['state'] == ('native_carried' if present else 'unresolved')
    assert record['semantic_confirmed'] is False
    assert bool([i for i in compiled['issues'] if i['code'] == 'outline_directory_material_carrier_unresolved']) != present


def test_public_pending_issue_uses_physical_location_without_dumping_candidate_records():
    compiled, documents = carrier_case()
    module = closure()
    carriers = module.candidate_carriers(compiled, documents)
    carriers[0]['origins'][0]['source_record']['original_text'] = 'WHOLE-CANDIDATE-BODY-SENTINEL'
    apply_directory_adjudications(compiled, documents, material_reply(compiled, documents)['directory_adjudications'])
    module.apply_carrier_closure(compiled, documents,
        [{'table_id': carriers[0]['table_id'], 'action': 'unresolved'}], carriers)
    detail = next(i['detail'] for i in compiled['issues'] if i['code'] == 'outline_directory_material_carrier_unresolved')
    assert 'WHOLE-CANDIDATE-BODY-SENTINEL' not in detail and 'source_windows' not in detail
    assert '物理第1页' in detail and '原表' in detail
    assert compiled['_source_carrier_closure'][0]['origins'][0]['source_record']['original_text'] == 'WHOLE-CANDIDATE-BODY-SENTINEL'
