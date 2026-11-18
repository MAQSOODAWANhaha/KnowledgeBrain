"""Fresh source-only intents bind parser code and effective native settings."""

from hashlib import sha256
from io import BytesIO
from uuid import uuid4

from pypdf import PdfWriter
from psycopg.types.json import Jsonb

from docreader.parser import pdf_parser, source_contract
from docreader.tender import evidence
from test_web_bridge import configured, finish_outline, payload, runtime as runtime, upload


def frozen(run):
    return next(row for row in run['artifacts'] if row['stage'] == 'freeze' and not row['scope'])


async def test_new_outline_upgrades_same_pdf_without_overwriting_old_freeze(runtime, monkeypatch):
    configured(monkeypatch)
    writer = PdfWriter()
    writer.add_blank_page(width=600, height=800)
    output = BytesIO()
    writer.write(output)
    data, document_id = output.getvalue(), str(uuid4())
    bridge = runtime.bridge
    async with bridge.store.transaction() as conn:
        await conn.execute('INSERT INTO tender_documents VALUES(%s,%s,%s,%s,%s,%s)',
            (document_id, runtime.project, 'tender.PDF', 'application/pdf', sha256(data).hexdigest(), len(data)))
    await bridge.put_document(payload(runtime, 'document.put', document_id=document_id), data)

    current = await bridge.source_run(runtime.project)
    prior_source = bridge.artifacts.read_json(frozen(current)['artifact_ref'])
    document, = prior_source['documents']
    raw = bridge.artifacts.read_json(document['raw_ref'])
    raw['source_contract'].pop('literal_source_profile')
    document['raw_ref'] = bridge.artifacts.put_json(raw)
    descriptor = bridge.artifacts.read_json(document['source_index_ref'])
    descriptor['raw_ref'] = document['raw_ref']
    document['source_index_ref'] = bridge.artifacts.put_json(descriptor)
    prior_ref = bridge.artifacts.put_json(prior_source)
    analysis = await bridge.store.submit(current['manifest_ref'], 'older-analysis-freeze')
    async with bridge.store.transaction() as conn:
        await conn.execute('INSERT INTO tender_project_analyses(run_id,project_id,request_key) VALUES(%s,%s,%s)',
            (analysis['run_id'], runtime.project, 'older-analysis'))
        await conn.execute('INSERT INTO tender_analysis_artifacts '
            '(run_id,source_version,domain_revision,stage,artifact_ref,epoch,metadata) '
            "VALUES(%s,1,0,'freeze',%s,0,'{}')", (analysis['run_id'], Jsonb(prior_ref)))

    prior_run = await bridge.store.get_run(analysis['run_id'])
    assert await bridge.sources_current(prior_run, await bridge.manifest(runtime.project))
    with monkeypatch.context() as legacy:
        check_sources = bridge.sources_current

        async def identity_only(run, registered, **_):
            return await check_sources(run, registered)

        legacy.setattr(bridge, 'sources_current', identity_only)
        previous, _, outline = await finish_outline(runtime, legacy, key='older-pdf-outline')
    assert outline['authoring_input']['frozen_ref'] == prior_ref
    started = await bridge.start_outline(payload(runtime, request_key='fresh-pdf-outline'))
    job = await bridge.job(runtime.project, started['generation']['execution_id'])
    fixed = bridge.artifacts.read_json(job['payload']['authoring_input_ref'])
    assert fixed['frozen_ref'] == frozen(current)['artifact_ref']
    assert job['run_id'] == current['run_id']
    assert frozen(await bridge.store.get_run(prior_run['run_id']))['artifact_ref'] == prior_ref
    assert await bridge.sources_current(current, await bridge.manifest(runtime.project), require_current_pdf=True)

    revised = await bridge.start_outline(payload(runtime, request_key='older-pdf-outline-revision',
                                                 previous_execution_id=previous))
    pinned = await bridge.job(runtime.project, revised['generation']['execution_id'])
    assert bridge.artifacts.read_json(pinned['payload']['authoring_input_ref'])['frozen_ref'] == prior_ref


async def test_same_profile_reuses_the_exact_run_without_parsing_again(runtime, monkeypatch):
    await upload(runtime)
    first = await runtime.bridge.source_run(runtime.project)

    async def unexpected_parse(*_args, **_options):
        raise AssertionError('unchanged source/profile must reuse its existing immutable freeze')

    monkeypatch.setattr(evidence, 'freeze_sources', unexpected_parse)
    assert await runtime.bridge.source_run(runtime.project) == first


async def test_parser_revision_creates_a_new_freeze_and_preserves_the_old_one(runtime, monkeypatch):
    await upload(runtime)
    first = await runtime.bridge.source_run(runtime.project)
    reference = frozen(first)['artifact_ref']
    original = runtime.bridge.artifacts.read_bytes(reference)
    previous = source_contract.parser_version()
    monkeypatch.setattr(source_contract, 'parser_version', lambda: previous + '/test-new-parser')
    second = await runtime.bridge.source_run(runtime.project)
    assert second['run_id'] != first['run_id']
    assert frozen(second)['metadata']['native_source_profile']['parser_version'] == previous + '/test-new-parser'
    assert runtime.bridge.artifacts.read_bytes(reference) == original
    assert await runtime.bridge.store.get_run(first['run_id']) == first


async def test_effective_native_setting_creates_a_new_freeze_for_the_same_manifest(runtime, monkeypatch):
    await upload(runtime)
    first = await runtime.bridge.source_run(runtime.project)
    old_ratio = pdf_parser.WORD_GAP_WIDTH_RATIO
    monkeypatch.setattr(pdf_parser, 'WORD_GAP_WIDTH_RATIO', old_ratio + 0.05)
    second = await runtime.bridge.source_run(runtime.project)
    assert second['run_id'] != first['run_id']
    profile = frozen(second)['metadata']['native_source_profile']
    assert profile['settings']['pdf']['word_gap_width_ratio'] == old_ratio + 0.05
    assert frozen(first)['metadata']['native_source_profile']['settings']['pdf']['word_gap_width_ratio'] == old_ratio
