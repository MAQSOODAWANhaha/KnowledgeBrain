"""Persisted directory requests and contracts retain their distinct identities."""

import json
from uuid import uuid4

import httpx
import pytest

from docreader.integrations.authoring_store import AuthoringStore
from docreader.integrations.authoring import read_authoring_input
from docreader.integrations.store import Conflict
from docreader.tender.document_execution import SourceOutlineUnavailable, execute_document

from test_analysis import analysis_runtime as analysis_runtime, execute_analysis
from test_authoring_execution import config, response
from test_authoring_outline import budget
from docreader.tests.workflow.test_web_bridge import runtime as runtime


pytestmark = pytest.mark.integration


def usable_outline(snapshot):
    return {'schema_version': 2, 'authoring_input': snapshot, 'issues': [], 'chapters': [{
        'chapter_id': 'chapter-1', 'title': '投标函', 'children': [], 'targets': [{
            'target_id': 'chapter-1:target-1', 'kind': 'form', 'purpose': '企业信息待人工填写',
            'rule_refs': [], 'source_refs': [], 'manual_fill': True, 'support_origins': ['tender']}]}]}


@pytest.mark.parametrize('lose_after_commit', [False, True])
async def test_layout_spec_lease_and_final_refs_are_durable_without_model_calls(
        analysis_runtime, monkeypatch, lose_after_commit):
    from docreader.tender import document_execution
    from docreader.tender.schema import AuthoringDocument

    store, artifacts, settings, run, _ = analysis_runtime
    await execute_analysis(analysis_runtime)
    seen, lost = [], []

    class Renderer:
        def spec(self):
            return {'contract': 'synthetic_layout_v1', 'api_origin': 'http://fixture.invalid'}

        def render(self, blob, *, epoch):
            seen.append((blob, epoch))
            return b'%PDF-fixture', 'fixture-engine'

    async def generate(artifacts, fixed, _budget, _execute, **options):
        outline_ref = artifacts.put_json({'schema_version': 2, 'authoring_input': fixed,
                                          'chapters': [], 'issues': []})
        cached = options.get('saved_layout_exports', {}).get('1' * 64)
        if cached is not None:
            exported = artifacts.read_json(cached)
            docx_ref, package_ref = exported['docx_ref'], exported['package_ref']
        else:
            assert options['render_pdf'](b'candidate') == (b'%PDF-fixture', 'fixture-engine')
            docx_ref = artifacts.put_bytes(b'candidate', 'application/vnd.openxmlformats-officedocument.wordprocessingml.document')
            package_ref = artifacts.put_bytes(b'package', 'application/zip')
            layout_ref = artifacts.put_json({'contract': 'toc_layout_v1', 'status': 'fixed', 'documents': [
                {'artifact_ref': docx_ref, 'status': 'fixed'}]})
            await options['on_layout']({'layout_ref': layout_ref, 'outline_ref': outline_ref,
                'docx_ref': docx_ref, 'package_ref': package_ref, 'export_input_digest': '1' * 64,
                'export_result_ref': artifacts.put_json({'docx_ref': docx_ref, 'package_ref': package_ref,
                    'layout_ref': layout_ref, 'gaps': [], 'attachments': []})})
            if lose_after_commit:
                lost.append(True)
                raise RuntimeError('executor lost after layout commit')
        empty = artifacts.put_json({})
        return artifacts.put_json(AuthoringDocument(authoring_input=fixed, outline_ref=outline_ref,
            basis_ref=empty, review_ref=empty, unit_refs=[], docx_ref=docx_ref, package_ref=package_ref,
            gaps=[], usage={}).model_dump())

    monkeypatch.setattr(document_execution, 'generate_document', generate)
    execution_id = uuid4().hex
    options = dict(execution_id=execution_id, run_id=run['run_id'], source_version=1,
                   domain_revision=1, layout_renderer=Renderer(),
                   transport=httpx.MockTransport(lambda request: pytest.fail('Layout does not call a model.')))
    if lose_after_commit:
        with pytest.raises(RuntimeError, match='executor lost'):
            await execute_document(store, artifacts, config(), budget(), **options)
    result_ref = await execute_document(store, artifacts, config(), budget(), **options)
    row = await AuthoringStore(settings.dsn).get_execution(execution_id)
    assert seen == [(b'candidate', 1)] and row['epoch'] == (2 if lose_after_commit else 1)
    assert bool(lost) is lose_after_commit
    assert row['execution_spec']['document_layout'] == Renderer().spec()
    assert 'epoch' not in row['execution_spec']['document_layout']
    receipt = next(r for r in row['receipts'] if r['stage'] == 'document-layout')
    assert receipt['scope'] == '1' * 64
    saved = artifacts.read_json(receipt['artifact_ref'])
    result = artifacts.read_json(result_ref)
    assert saved['docx_ref'] == result['docx_ref'] and saved['package_ref'] == result['package_ref']
    assert saved['outline_ref'] == result['outline_ref']
    assert artifacts.read_json(saved['layout_ref'])['documents'][0]['artifact_ref'] == result['docx_ref']
    assert await execute_document(store, artifacts, config(), budget(), **options) == result_ref
    assert len(seen) == 1 and row['requests'] == []


async def test_reviewed_facts_and_host_closure_are_pinned_and_inherited_without_model_calls(analysis_runtime, monkeypatch):
    from docreader.tender import document_execution
    from docreader.tender.directory_compile import documents_from_frozen
    from docreader.tender.directory_sources import SourceRefs

    store, artifacts, settings, run, _ = analysis_runtime
    await execute_analysis(analysis_runtime)
    snapshot = await read_authoring_input(store, artifacts, run_id=run['run_id'], source_version=1, domain_revision=1)
    documents = documents_from_frozen(artifacts, artifacts.read_json(snapshot['frozen_ref']))
    facts_ref = artifacts.put_json({'schema_version': 1, 'contract': 'reviewed_source_facts_v1',
        'authoring_input': snapshot, 'source_projection_digest': SourceRefs(documents).digest, 'facts': []})
    observed = []
    records = [{'table_id': 'synthetic-physical-table', 'state': 'native_carried', 'semantic_confirmed': False}]

    async def generate(artifacts, fixed, _budget, _execute, **options):
        observed.append(options['reviewed_source_facts_ref'])
        await options['on_source_closure'](records)
        reference = artifacts.put_json(usable_outline(fixed))
        await options['on_reviewed_facts'](reference, [])
        return reference

    def no_model(_request):
        pytest.fail('Source fact inheritance and host ledger storage do not send a model request.')

    monkeypatch.setattr(document_execution, 'generate_document', generate)
    first, second = uuid4().hex, uuid4().hex
    options = dict(run_id=run['run_id'], source_version=1, domain_revision=1, outline_only=True,
                   transport=httpx.MockTransport(no_model))
    await execute_document(store, artifacts, config(), budget(), execution_id=first,
                           reviewed_source_facts_ref=facts_ref, **options)
    await execute_document(store, artifacts, config(), budget(), execution_id=second,
                           previous_execution_id=first, **options)
    assert observed == [facts_ref, facts_ref]
    authoring = AuthoringStore(settings.dsn)
    for identity in (first, second):
        row = await authoring.get_execution(identity)
        assert row['execution_spec']['reviewed_source_facts_ref'] == facts_ref
        assert row['execution_spec']['reviewed_source_facts_contract'] == 'reviewed_source_facts_v1'
        ledger = next(receipt for receipt in row['receipts'] if receipt['stage'] == 'directory-material-closure')
        stored = artifacts.read_json(ledger['artifact_ref'])
        assert stored['records'] == records and stored['reviewed_source_facts_ref'] == facts_ref
        assert stored['authoring_input'] == snapshot
        applications = next(receipt for receipt in row['receipts'] if receipt['stage'] == 'directory-reviewed-source-facts')
        final = artifacts.read_json(applications['artifact_ref'])
        assert final['contract'] == 'reviewed_source_fact_applications_v1'
        assert final['records'] == [] and final['reviewed_source_facts_ref'] == facts_ref
        assert final['authoring_input'] == snapshot and final['outline_ref'] == row['result_ref']
        assert not [receipt for receipt in row['receipts'] if receipt['stage'] == 'document-model-response']
        changed = {**row['execution_spec'], 'reviewed_source_facts_ref': artifacts.put_json({'changed': True})}
        with pytest.raises(Conflict):
            await authoring.submit(identity, snapshot, changed)


async def test_directory_contract_and_both_reads_are_persisted_before_recovery(analysis_runtime):
    store, artifacts, settings, run, _ = analysis_runtime
    await execute_analysis(analysis_runtime)
    execution_id, sent = uuid4().hex, []

    def handler(request):
        sent.append(json.loads(json.loads(request.content)['messages'][1]['content'][0]['text']))
        return response({'nodes': [], 'composition': {'priority_basis': [], 'entries': []},
                         'scopes': [], 'excluded': [], 'source_only': []})

    options = dict(execution_id=execution_id, run_id=run['run_id'], source_version=1, domain_revision=1,
                   outline_only=True, transport=httpx.MockTransport(handler))
    with pytest.raises(SourceOutlineUnavailable):
        await execute_document(store, artifacts, config(), budget(), **options)
    row = await AuthoringStore(settings.dsn).get_execution(execution_id)
    assert row['execution_spec']['directory_planning_contract'] == 'source_refs_compact_plan_v12'
    assert row['execution_spec']['directory_material_identity_contract'] == 'material_short_ids_v3'
    assert row['execution_spec']['directory_parameter_source_contract'] == 'source_bound_parameter_tables_v1'
    assert row['execution_spec']['directory_response_form_contract'] == 'source_bound_response_forms_v3'
    assert row['execution_spec']['directory_source_group_contract'] == 'source_bound_submission_groups_v1'
    assert row['execution_spec']['directory_source_carrier_contract'] == 'source_carriers_closed_v1'
    assert [value['plan_slot'] for value in sent] == [1, 2]
    assert all(value['operation'] == 'select_directory_refs_v2'
               and value['source_ref_format'] == 'host_bound_line_refs_v1' for value in sent)
    receipts = [r for r in row['receipts'] if r['stage'] == 'document-model-response']
    assert len(receipts) == 2 and len({r['input_digest'] for r in receipts}) == 2
    assert row['status'] == 'failed' and row['result_ref'] is None
    rejected = next(r for r in row['receipts'] if r['stage'] == 'directory-outline-unavailable')
    assert artifacts.read_json(rejected['artifact_ref'])['chapters'] == []
    with pytest.raises(SourceOutlineUnavailable):
        await execute_document(store, artifacts, config(), budget(), **options)
    assert len(sent) == 2
    row = await AuthoringStore(settings.dsn).get_execution(execution_id)
    assert row['status'] == 'failed' and row['result_ref'] is None
    legacy_spec = {key: value for key, value in row['execution_spec'].items() if key != 'directory_planning_contract'}
    with pytest.raises(Conflict):
        await AuthoringStore(settings.dsn).submit(execution_id, row['authoring_input'], legacy_spec)
    assert await AuthoringStore(settings.dsn).get_execution(execution_id) == row
    old_material = {key: value for key, value in row['execution_spec'].items()
                    if key != 'directory_material_identity_contract'}
    with pytest.raises(Conflict):
        await AuthoringStore(settings.dsn).submit(execution_id, row['authoring_input'], old_material)
    assert await AuthoringStore(settings.dsn).get_execution(execution_id) == row
    for field in ('directory_parameter_source_contract', 'directory_response_form_contract', 'directory_source_group_contract',
                  'directory_source_carrier_contract'):
        changed = {key: value for key, value in row['execution_spec'].items() if key != field}
        with pytest.raises(Conflict):
            await AuthoringStore(settings.dsn).submit(execution_id, row['authoring_input'], changed)
        assert await AuthoringStore(settings.dsn).get_execution(execution_id) == row


@pytest.mark.parametrize('form_operation', ['select_native_response_form_v1', 'select_native_response_form_v2',
                                          'select_native_response_form_v3'])
async def test_native_form_task_is_durable_and_exact_predecessor_replay_does_not_send(analysis_runtime, monkeypatch, form_operation):
    from docreader.llm.contracts import response_model
    from docreader.llm.request import build_request
    from docreader.tender import document_execution

    store, artifacts, settings, run, _ = analysis_runtime
    await execute_analysis(analysis_runtime)
    payload = {'operation': form_operation, 'atom_count': 1,
               'form': {'grid': {'row_count': 2, 'column_count': 4}}, 'source_binding': 'synthetic-source-identity'}
    raw = {'dispositions': [{'refs': [1], 'kind': 'unresolved'}]}
    if form_operation in {'select_native_response_form_v2', 'select_native_response_form_v3'}:
        payload['mandate'] = [{'text': 'Preserve the declaration form.'}]
        raw = {'decision': 'keep_original', 'mandate_indices': [0], 'dispositions': []}
    sent = []

    async def generate(artifacts, snapshot, request_budget, execute, **options):
        assert options['response_form_contract'] == 'source_bound_response_forms_v3'
        assert options['parameter_source_contract'] == 'source_bound_parameter_tables_v1'
        assert options['source_group_contract'] == 'source_bound_submission_groups_v1'
        request = build_request(request_budget.model, request_budget.prompts['directory_response_form'],
                                payload, task='directory_response_form',
                                reasoning_effort=request_budget.reasoning_effort)
        await execute.plan('directory_response_form', 1, 'response-forms')
        response = await execute('directory_response_form', request,
                                 lambda value: response_model('directory_response_form', payload).model_validate(value).model_dump())
        assert response['dispositions'] == raw['dispositions']
        return artifacts.put_json(usable_outline(snapshot))

    def handler(request):
        sent.append(json.loads(request.content))
        return response(raw)

    monkeypatch.setattr(document_execution, 'generate_document', generate)
    first, second = uuid4().hex, uuid4().hex
    options = dict(run_id=run['run_id'], source_version=1, domain_revision=1,
                   outline_only=True, transport=httpx.MockTransport(handler))
    original = await execute_document(store, artifacts, config(), budget(), execution_id=first, **options)
    assert await execute_document(store, artifacts, config(), budget(), execution_id=first, **options) == original
    repeated = await execute_document(store, artifacts, config(), budget(), execution_id=second,
                                      previous_execution_id=first, **options)
    assert artifacts.read_json(repeated) == artifacts.read_json(original)
    assert len(sent) == 1
    first_row = await AuthoringStore(settings.dsn).get_execution(first)
    second_row = await AuthoringStore(settings.dsn).get_execution(second)
    assert len(first_row['requests']) == 1 and first_row['requests'][0]['task'] == 'directory_response_form'
    assert first_row['requests'][0]['status'] == 'succeeded'
    assert second_row['requests'] == []
    first_receipts = [r for r in first_row['receipts'] if r['stage'] == 'document-model-response']
    second_receipts = [r for r in second_row['receipts'] if r['stage'] == 'document-model-response']
    assert len(first_receipts) == len(second_receipts) == 1
    assert first_receipts[0]['scope'].startswith('directory_response_form:')
    assert first_receipts[0]['artifact_ref'] == second_receipts[0]['artifact_ref']
    assert second_receipts[0]['metadata']['reused_from_execution_id'] == first


@pytest.mark.parametrize('diagnostic_write_fails', [False, True])
async def test_empty_source_outline_fails_without_replacing_saved_revision_or_Word(
        runtime, monkeypatch, diagnostic_write_fails):
    import hashlib
    from docreader.integrations.web import execute_authoring_job
    from docreader.llm.config import ModelConfig
    from docreader.tender import document_execution
    from docreader.tests.workflow.test_web_bridge import finish_outline, outline_tree, payload, upload
    from docreader.tests.unit.test_saved_word_review import saved_bytes

    await upload(runtime)
    previous_execution, _, previous_outline = await finish_outline(runtime, monkeypatch)
    old_draft = (await runtime.bridge.save_outline(payload(runtime, request_key='old-outline',
        expected_revision_id=None, source_execution_id=previous_execution,
        chapters=outline_tree(previous_outline))))['draft']
    old_word = runtime.bridge.artifacts.put_bytes(saved_bytes(),
        'application/vnd.openxmlformats-officedocument.wordprocessingml.document')
    version = str(uuid4())
    async with runtime.bridge.store.transaction() as conn:
        # Word editor tables are outside the Python test-baseline sections.
        await conn.execute('CREATE TABLE tender_draft_versions(id uuid PRIMARY KEY, project_id uuid, '
            'source text, content_sha256 text, byte_length bigint)')
        await conn.execute('CREATE TABLE tender_draft_state(project_id uuid PRIMARY KEY, version_id uuid '
            'REFERENCES tender_draft_versions(id), session_id uuid, pending_save_id uuid)')
        await conn.execute("INSERT INTO tender_draft_versions VALUES(%s,%s,'manual',%s,%s)",
            (version, runtime.project, old_word['sha256'], old_word['size']))
        await conn.execute('INSERT INTO tender_draft_state VALUES(%s,%s,NULL,NULL)', (runtime.project, version))

    async def word_state():
        async with runtime.bridge.store.transaction() as conn:
            return await (await conn.execute('SELECT s.*,v.source,v.content_sha256,v.byte_length '
                'FROM tender_draft_state s JOIN tender_draft_versions v ON v.id=s.version_id '
                'WHERE s.project_id=%s', (runtime.project,))).fetchone()

    old_state = await word_state()
    monkeypatch.setattr(ModelConfig, 'from_env', classmethod(lambda cls: config()))
    generated = []

    async def generate(artifacts, snapshot, _budget, _execute, **options):
        reference = artifacts.put_json({'schema_version': 2, 'authoring_input': snapshot,
            'chapters': [], 'issues': [{'code': 'outline_directory_plan_failed',
                'detail': '原件目录读取未完成；原始响应和费用审计保留。', 'rule_refs': []}]})
        generated.append(reference)
        await options['on_outline'](reference)
        return reference

    actual_model = document_execution.OpenAIModel
    monkeypatch.setattr(document_execution, 'OpenAIModel', lambda *args, **kwargs: actual_model(
        *args, **{**kwargs, 'transport': httpx.MockTransport(
            lambda _request: pytest.fail('Publication failure must not send or retry a model request.'))}))
    monkeypatch.setattr(document_execution, 'generate_document', generate)
    if diagnostic_write_fails:
        original_commit = AuthoringStore.commit_artifact

        async def commit(self, lease, stage, *args, **kwargs):
            if stage == 'directory-outline-unavailable':
                raise OSError('synthetic diagnostic storage outage')
            return await original_commit(self, lease, stage, *args, **kwargs)

        monkeypatch.setattr(AuthoringStore, 'commit_artifact', commit)
    started = await runtime.bridge.start_outline(payload(runtime, request_key='empty-auto-outline'))
    execution_id = started['generation']['execution_id']
    await execute_authoring_job(execution_id, runtime.settings)
    row = await runtime.bridge.authoring.get_execution(execution_id)
    job = await runtime.bridge.job(runtime.project, execution_id)
    assert row['status'] == job['status'] == 'failed'
    assert row['result_ref'] is None and row['lease_until'] is None and row['requests'] == []
    expected_error = 'OSError' if diagnostic_write_fails else 'SourceOutlineUnavailable'
    assert row['error']['type'] == job['error']['type'] == expected_error
    rejected = [r for r in row['receipts'] if r['stage'] == 'directory-outline-unavailable']
    if diagnostic_write_fails:
        assert rejected == []
    else:
        assert len(rejected) == 1 and rejected[0]['artifact_ref'] == generated[0]
        assert rejected[0]['scope'] == generated[0]['sha256']
        assert rejected[0]['metadata']['error_type'] == 'SourceOutlineUnavailable'
    checkpoint = next(r for r in row['receipts'] if r['stage'] == 'document-outline')
    assert checkpoint['artifact_ref'] == generated[0]
    diagnostic = runtime.bridge.artifacts.read_json(generated[0])
    assert diagnostic['chapters'] == [] and diagnostic['issues'][0]['code'] == 'outline_directory_plan_failed'
    assert diagnostic['authoring_input'] == row['authoring_input']
    view = await runtime.bridge.outline_view(runtime.project)
    assert view['generation']['status'] == 'failed' and view['candidate']['execution_id'] == previous_execution
    assert view['draft'] == old_draft
    assert await word_state() == old_state
    async with runtime.bridge.store.transaction() as conn:
        versions = await (await conn.execute('SELECT count(*) AS count FROM tender_draft_versions WHERE project_id=%s',
            (runtime.project,))).fetchone()
    assert versions['count'] == 1
    assert hashlib.sha256(runtime.bridge.artifacts.read_bytes(old_word)).hexdigest() == old_word['sha256']
    assert len(generated) == 1
