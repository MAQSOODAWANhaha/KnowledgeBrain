"""Real SQL/stream/replay for source receipts; the small source scope is synthetic."""
from typing import Any, cast
from copy import deepcopy
from dataclasses import replace
import hashlib
import json
from uuid import uuid4

import httpx
import pytest

from docreader.integrations.authoring_store import AuthoringStore
from docreader.llm.budget import RequestBudget
from docreader.tender import authoring_pipeline, document_execution, native_body_scope
from docreader.tender.evidence.references import SourceIndex, fragment, selector
from docreader.tender.native_body_flow import SourceBodyResolutionPending
from docreader.tender.native_body_selection import CONTRACT, NativeBodySelectionView
from docreader.tender.schema import AuthoringInput
from docreader.tender.source_body import load_source_body

from test_analysis import analysis_runtime as analysis_runtime
from test_authoring_execution import config, response


pytestmark = pytest.mark.integration


def source_fixture(artifacts, run):
    text = '投标人应提交完整实施方案。'
    locator = {'locator_kind': 'document', 'section_ordinal': 0, 'page_ordinal': 0, 'heading_path': '实施方案'}
    raw = {'schema_version': 2, 'units': [{'unit_id': 'unit:0', 'kind': 'section', 'ordinal': 0,
        'text': text, 'grid': None, 'locator': locator, 'physical_path': 'native:unit:0', 'header_cells': []}],
        'source_contract': {'units': [{'unit_id': 'unit:0', 'section_id': 'section',
            'parent_section_id': None, 'heading_path': '实施方案', 'physical_locator': locator}], 'page_manifest': []},
        'images': {}, 'ocr': {}}
    document = {'document_id': 'document', 'document_version': 'one',
                'original_ref': artifacts.put_bytes(text.encode(), 'text/plain')}
    index = SourceIndex(document, raw)
    index_ref = artifacts.put_json(index.descriptor(artifacts.put_json(raw)))
    frozen = artifacts.put_json({'documents': [{**document, 'source_index_ref': index_ref, 'role': 'main'}]})
    snapshot = AuthoringInput(schema_version=2, frozen_ref=frozen,
        **{k: run[k] for k in ('run_id', 'source_version', 'domain_revision')}).model_dump()
    base = index.ranges[0]

    def excerpt(start, end):
        return {'source_ref': {'source_index_ref': index_ref, 'unit_id': base['unit_id']},
                'selectors': [selector(fragment(base, base['start'] + start, base['start'] + end))]}

    target = {'target_id': 'chapter-1:target-1', 'kind': 'narrative', 'purpose': text,
              'rule_refs': [], 'source_refs': [], 'source_excerpts': [excerpt(0, len(text))],
              'manual_fill': True, 'support_origins': ['tender']}
    outline = {'schema_version': 2, 'authoring_input': snapshot, 'issues': [],
        'chapters': [{'chapter_id': 'chapter-1', 'title': '实施方案', 'targets': [target], 'children': []}]}
    view = NativeBodySelectionView([{'cell_id': 1, 'source_kind': 'native_prose',
        'document_id': 'document', 'native_unit_id': base['unit_id'], 'native_range_id': base['range_id'],
        'page': 0, 'text': text}], [{'unit_id': 1, 'anchor_spans': [[1, 0, len(text)]],
                                    'islands': [{'island_id': 1, 'cell_ids': [1]}]}])

    class FixedScope:
        binding = hashlib.sha256(text.encode()).hexdigest()
        records = [{'target_id': target['target_id'], 'classification': 'native_resolution'}]
        bindings = {1: {'target_id': target['target_id'], 'chapter_id': 'chapter-1', 'layout_pending_rows': []}}
        exact_bindings = {}

        def selection_view(self):
            return view

        def request_payload(self, selected):
            return {'operation': 'synthetic_native_body_persistence', 'scope_binding': self.binding, **selected.payload()}

        def cell_excerpt(self, cell_id, start, end):
            assert cell_id == 1
            return excerpt(start, end)

    return snapshot, outline, FixedScope()


@pytest.mark.parametrize('interruption', ['receipt_ack_lost', 'source_pending'])
async def test_source_receipt_precedes_publication_and_survives_real_sql_recovery(
    analysis_runtime, monkeypatch, interruption
):
    analysis, artifacts, settings, run, _ = analysis_runtime
    snapshot, outline, scope = source_fixture(artifacts, run)
    cast(Any, scope).pending = ([{'target_id': 'chapter-1:target-1', 'reason': 'synthetic_source_review',
                       'outlet': 'manual_source_review'}] if interruption == 'source_pending' else [])
    monkeypatch.setattr(authoring_pipeline, 'outline_from_compiled', lambda *_args, artifacts: deepcopy(outline))
    monkeypatch.setattr(native_body_scope, 'NativeBodyScope', lambda *_args: scope)
    real_generate = authoring_pipeline.generate_document

    async def generate(*args, **kwargs):
        # Isolate the source-receipt boundary; the actual flow, executor, stream,
        # validators, source binding, publication callbacks and SQL remain real.
        return await real_generate(*args, directory_plans=[], directory_documents=[], **kwargs)

    monkeypatch.setattr(document_execution, 'generate_document', generate)
    commit = AuthoringStore.commit_artifact
    drop_ack = interruption == 'receipt_ack_lost'

    async def commit_with_lost_ack(self, lease, stage, reference, *args, **kwargs):
        nonlocal drop_ack
        saved = await commit(self, lease, stage, reference, *args, **kwargs)
        if stage == 'directory-native-body-sources' and drop_ack:
            drop_ack = False
            raise ConnectionError('synthetic receipt acknowledgement lost after SQL commit')
        return saved

    monkeypatch.setattr(AuthoringStore, 'commit_artifact', commit_with_lost_ack)
    sent = []

    def handler(request):
        wire = json.loads(request.content)
        assert wire['reasoning_effort'] == 'low'
        assert 'native_choices' in wire['messages'][0]['content']
        sent.append(wire)
        return response({'native_choices': [{'unit_id': 1, 'islands': [
            {'island_id': 1, 'state': 'selected', 'body': [[1, 1, 1, 0]], 'context': []}]}]})

    configured = config()
    configured = replace(configured, chat=replace(configured.chat, context_tokens=131072, reasoning_effort='low'))
    budget = RequestBudget(131072, model=configured.chat.model, tokenizer='o200k_base',
                           tokenized_authoring=True, reasoning_effort='low')
    execution_id = uuid4().hex
    pinned = artifacts.put_json(snapshot)

    async def execute():
        return await document_execution.execute_document(analysis, artifacts, configured, budget,
            execution_id=execution_id, **{k: run[k] for k in ('run_id', 'source_version', 'domain_revision')},
            outline_only=True, authoring_input_ref=pinned,
            directory_planning_contract='source_refs_compact_plan_v12',
            transport=httpx.MockTransport(handler))

    expected = ConnectionError if interruption == 'receipt_ack_lost' else SourceBodyResolutionPending
    with pytest.raises(expected):
        await execute()
    store = AuthoringStore(settings.dsn)
    first = await store.get_execution(execution_id)
    assert first['status'] == 'failed' and len(first['requests']) == len(sent) == 1
    assert first['requests'][0]['status'] == 'succeeded'
    assert not any(r['stage'] == 'document-outline' for r in first['receipts'])
    receipt, = [r for r in first['receipts'] if r['stage'] == 'directory-native-body-sources']
    saved = artifacts.read_json(receipt['artifact_ref'])
    assert saved['contract'] == CONTRACT and saved['authoring_input'] == snapshot
    assert saved['complete'] is (interruption != 'source_pending')
    candidate = artifacts.read_json(saved['outline_ref'])
    target = candidate['chapters'][0]['targets'][0]
    assert load_source_body(artifacts, snapshot, target) == '投标人应提交完整实施方案。'
    assert target['manual_fill'] and target['support_origins'] == ['tender']
    if interruption == 'source_pending':
        assert saved['pending'] and first['error']['type'] == 'SourceBodyResolutionPending'
        return

    result = await execute()
    final = await store.get_execution(execution_id)
    assert final['status'] == 'succeeded' and final['epoch'] == first['epoch'] + 1
    assert final['requests'] == first['requests'] and len(sent) == 1
    assert result == saved['outline_ref']
    stages = [r['stage'] for r in final['receipts']]
    assert stages.count('directory-native-body-sources') == stages.count('document-outline') == 1
    assert stages.index('directory-native-body-sources') < stages.index('document-outline')


async def test_joint_contract_is_stable_on_real_sql_retry(analysis_runtime, monkeypatch):
    from docreader.tender.native_requirement_sources import CONTRACT as JOINT_CONTRACT

    analysis, artifacts, settings, run, _ = analysis_runtime
    snapshot, outline, _ = source_fixture(artifacts, run)
    observed = []

    async def generate(*_args, **kwargs):
        observed.append(kwargs['native_requirement_contract'])
        assert 'native_body_contract' not in kwargs
        if len(observed) == 1:
            raise ConnectionError('synthetic lost execution after claiming lease')
        return artifacts.put_json(outline)

    monkeypatch.setattr(document_execution, 'generate_document', generate)
    configured = config()
    budget = RequestBudget(configured.chat.context_tokens, model=configured.chat.model)
    execution_id = uuid4().hex
    pinned = artifacts.put_json(snapshot)

    async def execute(**options):
        return await document_execution.execute_document(analysis, artifacts, configured, budget,
            execution_id=execution_id, **{k: run[k] for k in ('run_id', 'source_version', 'domain_revision')},
            outline_only=True, authoring_input_ref=pinned, **options)

    with pytest.raises(ConnectionError):
        await execute(native_requirement_contract=JOINT_CONTRACT)
    store = AuthoringStore(settings.dsn)
    first = await store.get_execution(execution_id)
    assert first['execution_spec']['native_requirement_contract'] == JOINT_CONTRACT
    assert 'directory_native_body_contract' not in first['execution_spec']
    await execute()
    final = await store.get_execution(execution_id)
    assert observed == [JOINT_CONTRACT, JOINT_CONTRACT]
    assert final['status'] == 'succeeded' and final['epoch'] == first['epoch'] + 1
