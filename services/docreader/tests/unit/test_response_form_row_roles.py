"""Original-table inclusion and the source clause's actor are separate decisions."""

from copy import deepcopy
import hashlib
import json
from io import BytesIO
from zipfile import ZipFile

from docx.oxml.ns import qn
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.contracts import response_model
from docreader.llm.request import ModelBudgetError, ModelContractError
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender import directory_response_forms as forms
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.authoring_structure import _structure_payload, review_structure
from docreader.tender.authoring_view import compact_structure_payload
from docreader.tender.directory_compile import outline_from_compiled
from docreader.tender.evidence.references import SourceIndex
from test_directory_compile import fixture_titles
from test_directory_response_forms import render_target, source_case, synthetic_response


CONTRACT = 'source_bound_response_forms_v3'
RECORD_CODE = 'outline_directory_response_form_row_roles'
PROCEDURE = '评标委员会按照失信情形扣除总得分的10%，不得由投标人自行计分。'


def role_case(tmp_path, *, contract=CONTRACT, merged_header=False):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled, selection, target = source_case(artifacts)
    frozen = artifacts.read_json(fixed['frozen_ref'])
    record = frozen['documents'][0]
    previous = record['source_index_ref']
    raw = artifacts.read_json(record['raw_ref'])
    if merged_header:
        grid = raw['units'][0]['grid']
        grid['cells'] = [cell for cell in grid['cells'] if (cell['row'], cell['column']) != (0, 1)]
        grid['cells'][0]['col_span'] = 2
    next(cell for unit in raw['units'] if unit['unit_id'] == 'end'
         for cell in unit['grid']['cells'] if cell['column'] == 4)['text'] = PROCEDURE
    index = SourceIndex({key: record[key] for key in ('document_id', 'document_version', 'original_ref')}, raw)
    raw_ref = artifacts.put_json(raw)
    current = artifacts.put_json(index.descriptor(raw_ref))

    def replace(value):
        if isinstance(value, dict):
            for key, entry in value.items():
                if key == 'source_index_ref' and entry == previous:
                    value[key] = deepcopy(current)
                else:
                    replace(entry)
        elif isinstance(value, list):
            for entry in value:
                replace(entry)

    replace([documents, compiled, target])
    record.update(source_index_ref=current, raw_ref=raw_ref)
    fixed['frozen_ref'] = artifacts.put_json(frozen)
    selection['table_id'] = forms.response_form_catalog(compiled, documents)[0]['table_id']
    compiled['_response_form_contract'] = contract
    view = forms.ResponseFormView(artifacts, compiled, documents, selection, fixed, contract=contract)
    return artifacts, fixed, compiled, target, view


def role_answer(view, role='procedure'):
    raw = synthetic_response(view.payload)
    program = next(ref for block in view.payload['source_scope'] for cell in block.get('cells', [])
                   for ref, text in cell['lines'] if text == PROCEDURE)
    raw['dispositions'][-1]['refs'].remove(program)
    raw['dispositions'].append({'refs': [program], 'kind': role})
    for row in raw['rows']:
        row['source_role'] = 'criterion'
    # Deliberately reversed; the host must bind roles after its actual source ordering.
    raw['rows'].insert(0, {'refs': [program], 'source_role': role})
    return {**raw, 'decision': 'expand', 'mandate_indices': [0]}


def bound_outline(artifacts, fixed, compiled, view, raw):
    compiled['_response_form_plans'] = [view.accept(raw)]
    return outline_from_compiled(fixed, compiled)


def test_only_v3_schema_requires_a_separate_role_on_each_selected_row():
    payload = {'atom_count': 10, 'mandate': [{}], 'form': {'grid': {'row_count': 7, 'column_count': 4}}}
    for operation in ['select_native_response_form_v1', 'select_native_response_form_v2', 'select_native_response_form_v3']:
        model = response_model('directory_response_form', {**payload, 'operation': operation})
        row = model.model_fields['rows'].annotation.__args__[0]
        assert ('source_role' in row.model_fields) == operation.endswith('v3')
        if operation.endswith('v3'):
            assert row.model_fields['source_role'].is_required()


@pytest.mark.parametrize('role', ['procedure', 'reference'])
@pytest.mark.parametrize('merged_header', [False, True])
def test_full_original_mandate_can_include_an_independent_non_bidder_row_with_blank_company_cells(tmp_path, role, merged_header):
    artifacts, fixed, compiled, _, view = role_case(tmp_path, merged_header=merged_header)
    accepted = view.accept(role_answer(view, role))
    assert accepted['complete']
    assert accepted['row_roles'] == ['criterion', role]
    assert not accepted['response_table']['source_guidance']
    assert all('source_role' not in row for row in accepted['response_table']['rows'])
    compiled['_response_form_plans'] = [accepted]
    outline = outline_from_compiled(fixed, compiled)
    target = outline['chapters'][0]['targets'][0]
    rows = target['response_table']['rows']
    record = json.loads(next(issue['detail'] for issue in outline['issues'] if issue['code'] == RECORD_CODE))
    assert record['response_form_contract'] == CONTRACT
    assert record['target_id'] == target['target_id']
    assert record['row_roles'] == [{'row_id': row['row_id'], 'row_ordinal': index, 'source_role': source_role}
                                   for index, (row, source_role) in enumerate(zip(rows, ['criterion', role]), 1)]
    assert record['mandate_scope'] == view.payload['mandate']
    document, _ = render_target(artifacts, target)
    assert len(document.tables) == 1 and len(document.tables[0].columns) == 4
    assert document.tables[0].cell(2, 1).text == PROCEDURE
    assert document.tables[0].rows[-1].cells[0].text == '……'
    assert len(list(document.tables[0]._tbl.iter(qn('w:gridSpan')))) == int(merged_header)
    assert all(not row.cells[2].text and not row.cells[3].text for row in document.tables[0].rows[1:])
    result = export_authoring_document(artifacts, outline, [])
    with ZipFile(BytesIO(artifacts.read_bytes(result['package_ref']))) as package:
        guide = package.read('提交要求与待办.md').decode()
    assert '第2行' in guide and ('评委或他方程序' if role == 'procedure' else '参考规则') in guide
    assert '企业事实未确认' in guide
    assert 'source_role' not in guide and 'row_roles' not in guide and record['source_binding'] not in guide


@pytest.mark.parametrize('problem', ['wrong_role', 'mixed_roles', 'duplicate_guidance', 'duplicate_row',
                                     'fake_response', 'fake_proof', 'missing_role'])
def test_non_bidder_rows_cannot_change_actor_repeat_source_or_supply_enterprise_facts(tmp_path, problem):
    _, _, _, _, view = role_case(tmp_path)
    raw = role_answer(view)
    row = raw['rows'][0]
    if problem == 'wrong_role':
        row['source_role'] = 'criterion'
    elif problem == 'mixed_roles':
        row['refs'] = sorted([*row['refs'], raw['rows'][1]['refs'][0]])
    elif problem == 'duplicate_guidance':
        raw['source_guidance'].append({'kind': 'procedure', 'refs': row['refs']})
    elif problem == 'duplicate_row':
        raw['rows'].append(deepcopy(row))
    elif problem == 'fake_response':
        row['response'] = '已经履行，评分100分。'
    elif problem == 'fake_proof':
        row['proof'] = '企业证明第1页。'
    else:
        row.pop('source_role')
    with pytest.raises(ModelContractError):
        view.accept(raw)


def test_old_v2_answer_cannot_be_automatically_accepted_as_v3(tmp_path):
    _, _, _, _, view = role_case(tmp_path)
    raw = role_answer(view)
    for row in raw['rows']:
        row.pop('source_role')
    with pytest.raises(ModelContractError):
        view.accept(raw)


@pytest.mark.parametrize('decision,indices', [('keep_original', [0]), ('unresolved', [])])
def test_v3_keeps_the_original_or_pending_exit_when_no_full_transcription_is_selected(tmp_path, decision, indices):
    artifacts, fixed, compiled, original, view = role_case(tmp_path)
    accepted = view.accept({'decision': decision, 'mandate_indices': indices})
    assert accepted['complete'] == (decision == 'keep_original')
    assert not accepted.get('row_roles')
    compiled['_response_form_plans'] = [accepted]
    outline = outline_from_compiled(fixed, compiled)
    assert not outline['chapters'][0]['targets'][0].get('response_table')
    original.pop('response_table')
    expected, _ = render_target(artifacts, original)
    actual, _ = render_target(artifacts, outline['chapters'][0]['targets'][0])
    assert actual.tables[0]._tbl.xml == expected.tables[0]._tbl.xml


@pytest.mark.parametrize('compact', [False, True])
async def test_structure_role_records_bind_actual_rows_and_change_review_request_digest(tmp_path, compact):
    artifacts, fixed, compiled, _, view = role_case(tmp_path)
    requests, payloads = [], []

    class ReviewBudget(RequestBudget):
        def require_request(self, request, task=None):
            sent = json.loads(request['messages'][1]['content'][0]['text'])
            if compact and sent.get('structure_view') != 'compact_v2':
                raise ModelBudgetError('Exercise the actual compact review request.')
            return super().require_request(request, task)

    for role in ['procedure', 'reference']:
        selected = deepcopy(compiled)
        outline = bound_outline(artifacts, fixed, selected, view, role_answer(view, role))
        payload, gaps = _structure_payload(artifacts, outline, [], outline_phase=True)
        assert not gaps
        if compact:
            payload = compact_structure_payload(payload)
        record = payload['response_form_decisions'][0]
        assert record['row_roles'][1]['source_role'] == role
        assert record['row_roles'][1]['review_row_id'] == 'q2'
        assert record['row_roles'][1]['row_id'] == outline['chapters'][0]['targets'][0]['response_table']['rows'][1]['row_id']
        assert record['mandate_scope'] == view.payload['mandate']
        payloads.append(payload)
        async def execute(task, request, validate):
            prompt = request['messages'][0]['content']
            assert 'source_role' in prompt and '企业' in prompt
            sent = json.loads(request['messages'][1]['content'][0]['text'])
            assert (sent.get('structure_view') == 'compact_v2') == compact
            assert sent['response_form_decisions'][0]['row_roles'][1]['source_role'] == role
            requests.append(request)
            return validate({'findings': []})
        await review_structure(artifacts, outline, [], ReviewBudget(model='fixture', context_tokens=131072),
                               execute, outline_phase=True)
    def digest(value):
        return hashlib.sha256(json.dumps(value, sort_keys=True).encode()).hexdigest()
    assert digest(payloads[0]) != digest(payloads[1])
    assert digest(requests[0]) != digest(requests[1])


def test_only_v3_request_uses_role_supplement_and_measures_its_larger_output_shape(tmp_path):
    _, _, _, _, view = role_case(tmp_path)
    budget = RequestBudget(model='fixture', context_tokens=131072, tokenizer='o200k_base', tokenized_authoring=True,
                           prompts={'directory_response_form': 'BASE', 'directory_response_form_v2': 'DECISION',
                                    'directory_response_form_v3': 'SOURCE ROW ROLE'})
    request = pipeline._request(budget, 'directory_response_form', view.payload)
    assert request['messages'][0]['content'].startswith('BASE\n\nDECISION\n\nSOURCE ROW ROLE')
    old_payload = {**view.payload, 'operation': 'select_native_response_form_v2'}
    assert forms.response_form_output_bound(view.payload, budget) > forms.response_form_output_bound(old_payload, budget)
    assert forms.require_response_form_budget(budget, request, view.payload)['worst_accepted_output_tokens'] <= 32768


async def test_v3_pipeline_makes_one_F_call_with_role_selection_and_retains_final_row_audit(tmp_path, monkeypatch):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled, selection, _ = source_case(artifacts)
    calls = []

    async def ask(_execute, _budget, task, payload, validate):
        calls.append(str(task))
        if task == 'directory_material':
            return validate({'chapter_titles': fixture_titles(payload), 'response_forms': [selection]})
        if task == 'directory_requirement':
            return validate({'assignments': []})
        if task == 'directory_classification':
            return validate({'items': []})
        assert task == 'directory_response_form' and payload['operation'] == 'select_native_response_form_v3'
        raw = synthetic_response(payload)
        for row in raw['rows']:
            row['source_role'] = 'criterion'
        return validate({**raw, 'decision': 'expand', 'mandate_indices': [0]})

    monkeypatch.setattr(pipeline, '_ask', ask)
    await pipeline._request_directory_details(compiled, documents, None, None, fixed, artifacts=artifacts,
                                              response_form_contract=CONTRACT)
    outline = outline_from_compiled(fixed, compiled)
    assert calls.count('directory_response_form') == 1
    assert outline['chapters'][0]['targets'][0]['response_table']['rows']
    assert any(issue['code'] == RECORD_CODE for issue in outline['issues'])


def test_a_stale_final_row_identity_keeps_role_review_explicitly_unbound(tmp_path):
    artifacts, fixed, compiled, _, view = role_case(tmp_path)
    outline = bound_outline(artifacts, fixed, compiled, view, role_answer(view))
    outline['chapters'][0]['targets'][0]['response_table']['rows'].reverse()
    payload, gaps = _structure_payload(artifacts, outline, [], outline_phase=True)
    assert any(gap['code'] == 'structure_response_form_role_unbound' for gap in gaps)
    assert not payload.get('response_form_decisions')


async def test_v3_budget_rejects_an_excessive_complete_output_before_execute(tmp_path):
    _, _, _, _, view = role_case(tmp_path)
    view.payload['atom_count'] = 100000
    budget = RequestBudget(model='fixture', context_tokens=131072, tokenizer='o200k_base', tokenized_authoring=True)

    async def execute(*args):
        pytest.fail('An unadmitted F request must not reach the model executor.')

    with pytest.raises(ModelBudgetError):
        await pipeline._ask(execute, budget, 'directory_response_form', view.payload, lambda raw: raw)
