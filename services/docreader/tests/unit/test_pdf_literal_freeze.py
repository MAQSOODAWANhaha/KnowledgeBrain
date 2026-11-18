"""Tender source references consume original glyphs before Markdown rendering."""
from copy import deepcopy
from hashlib import sha256
from pathlib import Path

import pytest

from docreader.integrations.artifacts import ArtifactIntegrityError, ArtifactStore
from docreader.tender.evidence import EvidenceRepository, freeze_sources
from docreader.tender.evidence.references import SourceIndex
from docreader.tender.native_body_prose import NativeProseSources
from test_source_presentation import pdf_bytes


async def freeze_pdf(tmp_path, lines, *, prefix=b''):
    artifacts = ArtifactStore(tmp_path)
    content = prefix + pdf_bytes(lines)
    original = artifacts.put_bytes(content, 'application/pdf')
    registration = {'documents': [{'document_id': 'literal', 'document_version': '1',
        'role': 'main', 'ordinal': 0, 'issues': [], 'file_name': 'literal.pdf',
        'original_ref': original}], 'issues': []}
    frozen = artifacts.read_json(await freeze_sources(registration, artifacts, plan=False))
    source, = frozen['documents']
    return artifacts, source, artifacts.read_json(source['raw_ref']), content


@pytest.mark.asyncio
async def test_pdf_freeze_indexes_literal_characters_and_keeps_real_hash_and_footer(tmp_path):
    lines = ['1. Original heading', '# Actual source hash', 'Signature: ______', '80']
    artifacts, source, raw, content = await freeze_pdf(tmp_path, lines)
    literal = artifacts.read_json(raw['literal_source_ref'])
    assert literal['source_sha256'] == sha256(content).hexdigest()
    assert literal['page_count'] == 1
    assert [c['char_index'] for c in literal['pages'][0]['characters']] == list(
        range(literal['pages'][0]['char_count']))
    index = EvidenceRepository(artifacts).source_index(source['source_index_ref'])
    actual = '\n'.join(r['text'] for r in index.ranges)
    assert '# Actual source hash' in actual
    assert 'Signature: ______' in actual and '80' in actual
    assert '1. Original heading' in actual
    assert '## 1. Original heading' not in actual
    assert all(r.get('text_basis') == 'pdf_literal' for r in index.ranges)


@pytest.mark.asyncio
async def test_literal_ranges_have_physical_glyph_mapping_and_materialize_exactly(tmp_path):
    artifacts, source, raw, _ = await freeze_pdf(tmp_path, ['Repeated text', 'Repeated text'])
    index = EvidenceRepository(artifacts).source_index(source['source_index_ref'])
    before = deepcopy(raw)
    assert raw['literal_units']
    literal = artifacts.read_json(raw['literal_source_ref'])
    for entry in index.ranges:
        mapping = raw['literal_units'][entry['unit_id']]
        chars = literal['pages'][mapping['page_ordinal']]['characters']
        selected = [chars[i]['text'] for i in mapping['char_indices']]
        assert entry['text'] == ''.join(selected)
        assert entry['physical_locator']['page_ordinal'] == mapping['page_ordinal']
        assert index.materialize(entry)['text'] == entry['text']
    assert artifacts.read_json(source['raw_ref']) == before


@pytest.mark.asyncio
async def test_legacy_prose_cannot_certify_one_pdf_literal_line_as_complete_body(tmp_path):
    lines = ['Supply the following:', 'Include the signed original.']
    artifacts, source, _, _ = await freeze_pdf(tmp_path, lines)
    repository = EvidenceRepository(artifacts)
    prose = NativeProseSources(repository, [source], {'literal': {'pages': ['\n'.join(lines)]}}, [])
    assert prose.match(lines[0], {('literal', 0)}) is None
    index = repository.source_index(source['source_index_ref'])
    assert [item['text'] for item in index.ranges] == lines


@pytest.mark.asyncio
async def test_legacy_prose_does_not_interpret_original_hash_as_markdown(tmp_path, monkeypatch):
    lines = ['# Sign here', 'Include the signed original.']
    artifacts, source, _, _ = await freeze_pdf(tmp_path, lines)
    repository = EvidenceRepository(artifacts)
    prose = NativeProseSources(repository, [source], {'literal': {'pages': ['\n'.join(lines)]}}, [])

    def fail(*args):
        pytest.fail('original PDF glyphs reached the legacy syntax projection')

    monkeypatch.setattr(prose, '_syntax_range', fail)
    assert prose.match(lines[0], {('literal', 0)}) is None
    index = repository.source_index(source['source_index_ref'])
    assert index.ranges[0]['text'] == lines[0]


@pytest.mark.asyncio
@pytest.mark.parametrize('tamper', ['wrong_original', 'reordered_characters', 'missing_mapping', 'missing_unit'])
async def test_literal_evidence_is_revalidated_when_loading_source_index(tmp_path, tamper):
    artifacts, source, raw, _ = await freeze_pdf(tmp_path, ['Original clause', 'Second mandatory clause'])
    identity = next(iter(raw['literal_units']))
    if tamper == 'wrong_original':
        literal = artifacts.read_json(raw['literal_source_ref'])
        literal['source_sha256'] = '0' * 64
        raw['literal_source_ref'] = artifacts.put_json(literal)
    elif tamper == 'reordered_characters':
        raw['literal_units'][identity]['char_indices'].reverse()
    elif tamper == 'missing_mapping':
        del raw['literal_units'][identity]
    else:
        del raw['literal_units'][identity]
        raw['units'] = [unit for unit in raw['units'] if unit['unit_id'] != identity]
        raw['source_contract']['units'] = [unit for unit in raw['source_contract']['units']
                                           if unit['unit_id'] != identity]
        for ordinal, unit in enumerate(raw['units']):
            unit['ordinal'] = ordinal
        for ordinal, unit in enumerate(raw['source_contract']['units']):
            unit['ordinal'] = ordinal
        for page in raw['source_contract'].get('page_manifest', []):
            page['unit_ids'] = [unit for unit in page['unit_ids'] if unit != identity]
    original = artifacts.read_json(source['source_index_ref'])['document']
    raw_ref = artifacts.put_json(raw)
    # Rebuild the ordinary descriptor, so its range digest is internally valid.
    # The independent original-glyph binding must still reject the mutation.
    descriptor = SourceIndex(original, raw).descriptor(raw_ref)
    with pytest.raises(ArtifactIntegrityError):
        EvidenceRepository(artifacts).source_index(artifacts.put_json(descriptor))


@pytest.mark.asyncio
async def test_literal_freeze_uses_the_same_pdf_type_detection_as_native_parser(tmp_path):
    artifacts, source, raw, content = await freeze_pdf(
        tmp_path, ['Original with a PDF header offset'], prefix=b'% container preamble\n')
    literal = artifacts.read_json(raw['literal_source_ref'])
    assert literal['source_sha256'] == sha256(content).hexdigest()
    assert source['original_ref']['sha256'] == literal['source_sha256']
    assert any(unit.get('text_basis') == 'pdf_literal' for unit in raw['units'])


@pytest.mark.parametrize('file_name', ['native.py', 'freeze.py', 'packing.py', 'pdf_literal.py', 'references.py'])
def test_source_profile_tracks_literal_contract_and_each_actual_source_file(monkeypatch, file_name):
    from docreader.integrations.web import _native_source_profile
    from docreader.tender.evidence import references
    from docreader.tender.evidence.pdf_literal import CONTRACT

    before = _native_source_profile()
    assert before['literal_source_profile']['contract'] == CONTRACT
    evidence = Path(references.__file__).parent
    read = Path.read_bytes

    def changed(path):
        content = read(path)
        return content + b'\n# simulated source revision\n' if path == evidence / file_name else content

    monkeypatch.setattr(Path, 'read_bytes', changed)
    after = _native_source_profile()
    assert before['parser_version'] == after['parser_version']
    assert before['settings'] == after['settings']
    assert before['literal_source_profile'] != after['literal_source_profile']
    assert before['literal_source_profile']['sources'][file_name] != after['literal_source_profile']['sources'][file_name]


@pytest.mark.asyncio
async def test_literal_freeze_records_its_source_contract_profile(tmp_path):
    from docreader.tender.evidence.references import literal_source_profile

    _, _, raw, _ = await freeze_pdf(tmp_path, ['Original clause'])
    assert raw['source_contract']['literal_source_profile'] == literal_source_profile()
