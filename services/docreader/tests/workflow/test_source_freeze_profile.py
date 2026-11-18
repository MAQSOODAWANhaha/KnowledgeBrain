"""Fresh source-only intents bind parser code and effective native settings."""

from docreader.parser import pdf_parser, source_contract
from docreader.tender import evidence
from test_web_bridge import runtime as runtime, upload


def frozen(run):
    return next(row for row in run['artifacts'] if row['stage'] == 'freeze' and not row['scope'])


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
