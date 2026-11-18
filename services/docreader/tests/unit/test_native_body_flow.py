"""The final resolution stage selects references and preserves every pending domain."""
from copy import deepcopy
import json

import pytest

from docreader.llm.budget import RequestBudget
from docreader.tender.native_body_flow import resolve_native_bodies, selection_request
from docreader.tender.native_body_selection import NativeBodySelectionView
from docreader.tender.source_body import load_source_body
from test_native_body_context import source_case


class Scope:
    def __init__(self, target, *, mixed=False):
        self.binding, self.pending = 'fixed-scope', []
        self.records = [{'target_id': target['target_id'], 'classification': 'native_selection'}]
        self.bindings = {1: {'target_id': target['target_id'], 'chapter_id': 'chapter-1',
                             'layout_pending_rows': [{'line': 1}] if mixed else []}}
        self.excerpts = {1: deepcopy(target['source_context_excerpts'][0]),
                         2: deepcopy(target['source_context_excerpts'][1]),
                         3: deepcopy(target['source_excerpts'][0])}
        self.view = NativeBodySelectionView([
            {'cell_id': i, 'document_id': 'document', 'table_id': 1,
             'row': 0, 'column': i - 1, 'text': text}
            for i, text in enumerate(('建设方案评价', '5', '投标人应提交完整实施方案。'), 1)],
            [{'unit_id': 1, 'islands': [{'island_id': 1, 'cell_ids': [1, 2, 3]}]}])

    def selection_view(self):
        return self.view

    def request_payload(self, view):
        return {'operation': 'select_final_native_body_sources_v2',
                'scope_binding': self.binding, **view.payload()}

    def cell_excerpt(self, cell_id, start, end):
        assert start == 0 and end == len(self.view.by_cell[cell_id]['text'])
        return deepcopy(self.excerpts[cell_id])


def budget():
    return RequestBudget(131072, model='grok', tokenizer='o200k_base', tokenized_authoring=True)


@pytest.mark.asyncio
@pytest.mark.parametrize('state', ['selected', 'unresolved', 'mixed'])
async def test_resolution_binds_exact_body_or_preserves_the_original_pending_target(tmp_path, state):
    artifacts, outline, target = source_case(tmp_path)
    scope = Scope(target, mixed=state == 'mixed')
    target.pop('source_body_ref')
    target.pop('source_body_contract')
    target['purpose'] = '建设方案评价 5 投标人应提交完整实施方案。'
    before = deepcopy(outline)
    sent = []

    async def execute(task, request, validate):
        sent.append((task, request))
        payload = json.loads(request['messages'][1]['content'][0]['text'])
        assert payload['scope_binding'] == scope.binding
        assert 'prefixItems' in request['messages'][0]['content']
        body = [] if state == 'unresolved' else [[3, 1, 1, 0]]
        return validate({'native_choices': [{'unit_id': 1, 'islands': [
            {'island_id': 1, 'state': 'unresolved' if not body else 'selected', 'body': body}]}]})

    updated, receipt = await resolve_native_bodies(artifacts, outline['authoring_input'], outline,
                                                  scope, budget(), execute)
    assert outline == before and len(sent) == 1
    actual = updated['chapters'][0]['targets'][0]
    if state == 'selected':
        assert receipt['complete'] is True
        assert load_source_body(artifacts, updated['authoring_input'], actual) == '投标人应提交完整实施方案。'
        assert actual['manual_fill'] is True and actual['support_origins'] == ['tender']
        assert actual['source_context_excerpts'] == [scope.excerpts[1], scope.excerpts[2]]
    else:
        assert receipt['complete'] is False and receipt['pending']
        assert actual == target
        assert receipt['records'][0]['state'] == ('selected_pending_layout' if state == 'mixed' else 'unresolved')


def test_actual_complete_r_request_has_its_own_schema_and_no_provider_output_cap(tmp_path):
    _, _, target = source_case(tmp_path)
    scope = Scope(target)
    request = selection_request(budget(), scope, scope.selection_view())
    assert 'max_tokens' not in request and 'max_completion_tokens' not in request
    assert 'native_choices' in request['messages'][0]['content']
    assert 'source_rows' in request['messages'][0]['content']
    assert budget().require_request(request, 'directory_parameter')['fits'] is True


@pytest.mark.asyncio
@pytest.mark.parametrize('complete', [False, True])
async def test_source_receipt_is_saved_before_publication_and_pending_cannot_publish(tmp_path, monkeypatch, complete):
    from docreader.tender import authoring_pipeline as pipeline
    from docreader.tender import native_body_flow, native_body_scope
    from docreader.tender.native_body_flow import SourceBodyResolutionPending
    from docreader.tender.native_body_selection import CONTRACT
    artifacts, original, _ = source_case(tmp_path)
    monkeypatch.setattr(pipeline, 'outline_from_compiled',
                        lambda snapshot, compiled, *, artifacts: deepcopy(original))
    monkeypatch.setattr(native_body_scope, 'NativeBodyScope', lambda *args, **kwargs: object())

    async def resolve(artifacts, snapshot, outline, *args):
        return outline, {'contract': CONTRACT, 'complete': complete, 'records': [],
                         'pending': [] if complete else [{'reason': 'source_review_required'}]}

    monkeypatch.setattr(native_body_flow, 'resolve_native_bodies', resolve)
    events = []

    async def save(reference):
        receipt = artifacts.read_json(reference)
        assert artifacts.read_json(receipt['outline_ref'])['chapters'] == original['chapters']
        assert receipt['authoring_input']['frozen_ref'] == original['authoring_input']['frozen_ref']
        events.append('receipt')

    async def publish(reference):
        assert events == ['receipt']
        events.append('outline')

    async def no_calls(*args, **kwargs):
        pytest.fail('this host publication check must not send a model request')

    operation = pipeline.generate_document(artifacts, original['authoring_input'], budget(), no_calls,
        directory_plans=[], directory_documents=[], outline_only=True, native_body_contract=CONTRACT,
        on_source_bodies=save, on_outline=publish)
    if complete:
        await operation
        assert events == ['receipt', 'outline']
    else:
        with pytest.raises(SourceBodyResolutionPending):
            await operation
        assert events == ['receipt']
