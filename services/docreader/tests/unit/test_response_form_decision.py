"""An original response form may need preservation rather than source transcription."""

from copy import deepcopy
import json

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.contracts import response_model
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender import directory_response_forms as forms
from docreader.tender.directory_compile import outline_from_compiled
from test_directory_compile import fixture_titles
from test_directory_response_forms import source_case, synthetic_response, render_target


CONTRACT = 'source_bound_response_forms_v2'


def decision_case(tmp_path, *, external_mandate=None):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled, selection, target = source_case(artifacts)
    documents[0]['pages'][0] = '原式\n请据实填写实际偏差及原因。'
    if external_mandate:
        documents[0]['pages'][0] += '\n' + external_mandate
        selection['mandate_windows'].append({**selection['mandate_windows'][0], 'start_line': 2, 'end_line': 2})
    compiled['chapters'][0]['text'] = '原式'
    view = forms.ResponseFormView(artifacts, compiled, documents, selection, fixed, contract=CONTRACT)
    compiled['_response_form_contract'] = CONTRACT
    compiled['_response_form_pending'] = [forms.pending_response_form(
        artifacts, compiled, documents, selection, fixed)]
    target.pop('response_table')
    return artifacts, compiled, target, view


def test_only_new_response_operation_advertises_the_mandate_decision():
    payload = {'atom_count': 10, 'mandate': [{'text': 'Actual declaration only.'}],
               'form': {'grid': {'row_count': 6, 'column_count': 5}}}
    legacy = response_model('directory_response_form', payload)
    modern = response_model('directory_response_form', {**payload, 'operation': 'select_native_response_form_v2'})
    assert 'decision' not in legacy.model_fields
    assert {'decision', 'mandate_indices'} <= modern.model_fields.keys()


def test_source_based_keep_original_retains_the_exact_empty_grid_and_records_basis(tmp_path):
    artifacts, compiled, target, view = decision_case(tmp_path)
    before, _ = render_target(artifacts, deepcopy(target))
    accepted = view.accept({'decision': 'keep_original', 'mandate_indices': [0]})
    assert accepted['complete'] and accepted['decision'] == 'keep_original'
    assert accepted['mandate_sources'] == view.payload['mandate']
    compiled['_response_form_plans'] = [accepted]
    forms.bind_response_form_targets(compiled, {'source-form': {'targets': [target]}})
    assert not target.get('response_table') and not target.get('response_form_pending')
    after, _ = render_target(artifacts, target)
    assert before.tables[0]._tbl.xml == after.tables[0]._tbl.xml
    decision = next(issue for issue in compiled['issues'] if issue['code'] == 'outline_directory_response_form_decision')
    assert view.payload['mandate'][0]['text'] in decision['detail']
    assert '企业事实未确认' in decision['detail']
    forms.bind_response_form_targets(compiled, {'source-form': {'targets': [target]}})
    assert sum(issue == decision for issue in compiled['issues']) == 1


@pytest.mark.parametrize('indices', [[], [0, 0], [1], [True]])
def test_keep_original_requires_known_unique_mandate_basis(tmp_path, indices):
    _, _, _, view = decision_case(tmp_path)
    with pytest.raises(ModelContractError):
        view.accept({'decision': 'keep_original', 'mandate_indices': indices})


@pytest.mark.parametrize('field', ['rows', 'layout', 'dispositions', 'contexts'])
def test_keep_original_cannot_smuggle_a_partial_expansion(tmp_path, field):
    _, _, _, view = decision_case(tmp_path)
    raw = {'decision': 'keep_original', 'mandate_indices': [0], field: synthetic_response(view.payload)[field]}
    with pytest.raises(ModelContractError):
        view.accept(raw)


def test_expansion_still_requires_complete_dispositions_and_exact_native_layout(tmp_path):
    _, _, _, view = decision_case(tmp_path)
    raw = {**synthetic_response(view.payload), 'decision': 'expand', 'mandate_indices': [0]}
    assert view.accept(raw)['complete']
    raw['dispositions'][-1]['refs'].pop()
    with pytest.raises(ModelContractError):
        view.accept(raw)


def test_unresolved_decision_preserves_the_form_and_pending_exit(tmp_path):
    _, compiled, target, view = decision_case(tmp_path)
    accepted = view.accept({'decision': 'unresolved', 'mandate_indices': []})
    assert not accepted['complete']
    compiled['_response_form_plans'] = [accepted]
    forms.bind_response_form_targets(compiled, {'source-form': {'targets': [target]}})
    assert target['response_form_pending'] and not target.get('response_table')


def test_old_partial_answer_is_not_a_new_mandate_decision(tmp_path):
    _, _, _, view = decision_case(tmp_path)
    with pytest.raises(ModelContractError):
        view.accept(synthetic_response(view.payload))


def test_expand_without_atom_dispositions_is_a_located_contract_error(tmp_path):
    _, _, _, view = decision_case(tmp_path)
    with pytest.raises(ModelContractError):
        view.accept({'decision': 'expand', 'mandate_indices': [0]})


async def test_new_pipeline_preserves_original_form_after_one_source_decision(tmp_path, monkeypatch):
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
        assert task == 'directory_response_form' and payload['operation'] == 'select_native_response_form_v2'
        return validate({'decision': 'keep_original', 'mandate_indices': [0]})

    monkeypatch.setattr(pipeline, '_ask', ask)
    await pipeline._request_directory_details(compiled, documents, None, None, fixed, artifacts=artifacts,
                                              response_form_contract=CONTRACT)
    outline = outline_from_compiled(fixed, compiled)
    target = outline['chapters'][0]['targets'][0]
    assert not target.get('response_table') and not target.get('response_form_pending')
    assert not any(issue['code'] == 'outline_directory_response_form_unconfirmed' for issue in outline['issues'])
    assert calls.count('directory_response_form') == 1


def test_v2_prompt_and_full_budget_are_distinct_while_v1_prompt_is_unchanged(tmp_path):
    _, _, _, view = decision_case(tmp_path)
    budget = RequestBudget(model='fixture', context_tokens=131072, tokenizer='o200k_base', tokenized_authoring=True,
                           prompts={'directory_response_form': 'BASE', 'directory_response_form_v2': 'MANDATE DECISION'})
    request = pipeline._request(budget, 'directory_response_form', view.payload)
    assert request['messages'][0]['content'].startswith('BASE\n\nMANDATE DECISION')
    measured = forms.require_response_form_budget(budget, request, view.payload)
    old_payload = {**view.payload, 'operation': 'select_native_response_form_v1'}
    old = pipeline._request(budget, 'directory_response_form', old_payload)
    assert 'MANDATE DECISION' not in old['messages'][0]['content']
    assert forms.response_form_output_bound(view.payload, budget) > forms.response_form_output_bound(old_payload, budget)
    assert measured['worst_accepted_output_tokens'] <= measured['output_reserve']


@pytest.mark.parametrize('compact', [False, True])
def test_review_sees_all_mandates_even_when_keep_original_only_selects_the_weaker_basis(tmp_path, compact):
    from docreader.tender.authoring_structure import _structure_payload
    from docreader.tender.authoring_view import compact_structure_payload

    forced = '投标人必须逐条列出全部评审要求原文。'
    artifacts, compiled, _, view = decision_case(tmp_path, external_mandate=forced)
    compiled['_response_form_plans'] = [view.accept({'decision': 'keep_original', 'mandate_indices': [0]})]
    fixed, _, _, _, _ = source_case(artifacts)
    outline = outline_from_compiled(fixed, compiled)
    assert forced not in outline['chapters'][0]['targets'][0]['purpose']
    payload = _structure_payload(artifacts, outline, [], outline_phase=True)[0]
    if compact:
        payload = compact_structure_payload(payload)
    decision = payload['response_form_decisions'][0]
    assert decision['target_id'] == 'v1' and 'keep_original' in decision['detail']
    assert forced in decision['detail'] and view.payload['mandate'][0]['text'] in decision['detail']


async def test_structure_prompt_explicitly_checks_the_original_form_decision_against_full_mandates(tmp_path):
    from docreader.tender.authoring_structure import review_structure

    artifacts = ArtifactStore(tmp_path)
    fixed, _, _, _, _ = source_case(artifacts)
    _, compiled, _, view = decision_case(tmp_path, external_mandate='本表须逐条列原文。')
    compiled['_response_form_plans'] = [view.accept({'decision': 'keep_original', 'mandate_indices': [0]})]
    outline = outline_from_compiled(fixed, compiled)
    budget = RequestBudget(model='fixture', context_tokens=131072, prompts={'authoring_structure': 'BASE STRUCTURE'})

    async def execute(task, request, validate):
        payload = json.loads(request['messages'][1]['content'][0]['text'])
        assert task == 'authoring_structure' and payload['response_form_decisions']
        assert 'keep_original' in request['messages'][0]['content']
        assert '全部' in request['messages'][0]['content']
        return validate({'findings': []})

    await review_structure(artifacts, outline, [], budget, execute, outline_phase=True)
