"""Real PostgreSQL saves retain source-only candidates through immutable revisions."""

from copy import deepcopy
from io import BytesIO
import hashlib
from uuid import uuid4

from docx import Document
import pytest

from docreader.integrations.authoring import read_authoring_input
from docreader.integrations.web import WebError
from docreader.integrations.web_outline import outline_detail
from docreader.tender.authoring_coverage import build_ledger
from test_web_bridge import configured, finish_outline, payload, upload
from test_web_bridge import runtime as runtime


async def upload_source_form(runtime):
    document = Document()
    table = document.add_table(rows=3, cols=2)
    table.cell(0, 0).merge(table.cell(0, 1)).text = '待填原表'
    table.cell(1, 0).text = '负责人'
    table.cell(2, 0).text = '证书编号'
    stream = BytesIO()
    document.save(stream)
    data, document_id = stream.getvalue(), str(uuid4())
    async with runtime.bridge.store.transaction() as conn:
        await conn.execute('INSERT INTO tender_documents VALUES(%s,%s,%s,%s,%s,%s)',
            (document_id, runtime.project, '原表.docx',
             'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
             hashlib.sha256(data).hexdigest(), len(data)))
    await runtime.bridge.put_document(payload(runtime, 'document.put', document_id=document_id), data)


async def test_source_only_form_removal_resave_body_snapshot_and_restore(runtime, monkeypatch):
    configured(monkeypatch)
    await upload_source_form(runtime)
    started = await runtime.bridge.start_outline(payload(runtime, request_key='source-form-candidate'))
    execution_id = started['generation']['execution_id']
    job = await runtime.bridge.job(runtime.project, execution_id)
    snapshot = await read_authoring_input(runtime.bridge.store, runtime.bridge.artifacts, **job['payload']['identity'])
    frozen = runtime.bridge.artifacts.read_json(snapshot['frozen_ref'])
    document = frozen['documents'][0]
    raw = runtime.bridge.artifacts.read_json(document['raw_ref'])
    native = next(unit for unit in raw['units'] if unit.get('grid'))
    reference = {'source_index_ref': document['source_index_ref'], 'unit_id': native['unit_id']}
    target = {'target_id': 'source-only-form', 'kind': 'form', 'purpose': '完整待填原表',
              'rule_refs': [], 'source_refs': [reference], 'template_refs': [
                  {'source_ref': reference, 'selectors': [], 'header_row_count': 1}], 'manual_fill': True}
    original = {'schema_version': 2, 'authoring_input': snapshot, 'issues': [], 'chapters': [
        {'chapter_id': 'form', 'title': '原式', 'targets': [target], 'children': []}]}
    original_ref = runtime.bridge.artifacts.put_json(original)
    await runtime.bridge.authoring.submit(execution_id, snapshot, {'protocol': 'outline-test'})
    lease = await runtime.bridge.authoring.acquire(execution_id)
    await runtime.bridge.authoring.release(lease, 'succeeded', result_ref=original_ref)
    first = (await runtime.bridge.save_outline(payload(runtime, request_key='remove-source-form',
        expected_revision_id=None, source_execution_id=execution_id, chapters=[])))['draft']
    assert first['outline']['chapters'] == []
    assert first['outline']['unplaced_targets'] == [target]
    assert build_ledger(first['outline'])[0]['resolution'] == 'pending'
    second = (await runtime.bridge.save_outline(payload(runtime, request_key='resave-source-form',
        expected_revision_id=first['revision_id'], source_revision_id=first['revision_id'], chapters=[])))['draft']
    assert second['outline']['unplaced_targets'] == [target]
    submitted = await runtime.bridge.start_authoring(payload(runtime, request_key='body-from-missing-source-form',
        outline_revision_id=second['revision_id'], outline_sha256=second['sha256']))
    body_job = await runtime.bridge.job(runtime.project, submitted['authoring']['execution_id'])
    fixed = runtime.bridge.artifacts.read_json(body_job['payload']['outline_ref'])
    assert fixed['unplaced_targets'] == [target] and fixed['chapters'] == []
    detail = await outline_detail(runtime.bridge, runtime.project, second['revision_id'])
    assert detail['available_targets'] == [target]
    restored = (await runtime.bridge.save_outline(payload(runtime, request_key='restore-source-form',
        expected_revision_id=second['revision_id'], source_revision_id=second['revision_id'], chapters=[
            {'chapter_id': 'manual-' + str(uuid4()), 'title': '新位置',
             'target_ids': [target['target_id']], 'children': []}])))['draft']
    assert restored['outline']['unplaced_targets'] == []
    assert restored['outline']['chapters'][0]['targets'] == [target]
    assert build_ledger(restored['outline'])[0]['resolution'] == 'manual'
    assert runtime.bridge.artifacts.read_json(original_ref) == original


async def test_unplaced_snapshot_is_server_owned_and_other_execution_is_not_merged(runtime, monkeypatch):
    await upload(runtime)
    first_execution, _, _ = await finish_outline(runtime, monkeypatch, key='unplaced-first')
    first = (await runtime.bridge.save_outline(payload(runtime, request_key='unplaced-save-first',
        expected_revision_id=None, source_execution_id=first_execution, chapters=[])))['draft']
    assert len(first['outline']['unplaced_targets']) == 1
    second_execution, _, second_outline = await finish_outline(runtime, monkeypatch,
        key='unplaced-second', target_id='different-generation')
    second = (await runtime.bridge.save_outline(payload(runtime, request_key='unplaced-save-second',
        expected_revision_id=first['revision_id'], source_execution_id=second_execution, chapters=[])))['draft']
    assert second['outline']['unplaced_targets'] == second_outline['chapters'][0]['targets']
    forged = deepcopy(first['outline']['unplaced_targets'])
    with pytest.raises(WebError, match='响应|来源|快照'):
        await runtime.bridge.save_outline(payload(runtime, request_key='forged-unplaced',
            expected_revision_id=second['revision_id'], source_revision_id=second['revision_id'],
            chapters=[], unplaced_targets=forged))
    assert (await runtime.bridge.outline_view(runtime.project))['draft']['revision_id'] == second['revision_id']
