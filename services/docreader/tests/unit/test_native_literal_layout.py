from copy import deepcopy

import pytest

from docreader.tender.directory_compile import _FrozenLayout, documents_from_frozen
from docreader.tender.evidence.references import EvidenceRepository
from test_pdf_literal_freeze import freeze_pdf


@pytest.mark.asyncio
async def test_literal_layout_binds_unique_whole_original_line(tmp_path):
    from docreader.tender.native_requirement_flow import literal_layout_locations

    lines = ['Sign # 2.', 'Repeated words', 'Repeated words', 'A complete unbound line.']
    artifacts, frozen, _, _ = await freeze_pdf(tmp_path, lines)
    document, = documents_from_frozen(artifacts, {'documents': [frozen]})
    index = EvidenceRepository(artifacts).source_index(frozen['source_index_ref'])
    units = [unit for unit in index.raw['units'] if unit['text_basis'] == 'pdf_literal']
    heading, *_, tail = units
    position = next(i for i, line in enumerate(document['pages'][0].splitlines()) if heading['text'] in line)
    anchor = {'document_id': 'literal', 'page': 0, 'line': position}
    locations = literal_layout_locations(artifacts, index, document)
    assert locations[heading['unit_id']] == anchor
    assert tail['unit_id'] in locations
    assert len(locations) == 2

    modified = deepcopy(document)
    modified['pages'][0] = modified['pages'][0].replace(tail['text'], 'A complete')
    assert literal_layout_locations(artifacts, index, modified) == {}

    _, other_frozen, _, _ = await freeze_pdf(tmp_path, lines, prefix=b'%PDF source revision\n')
    other, = documents_from_frozen(artifacts, {'documents': [other_frozen]})
    assert other['document_id'] == document['document_id']
    assert other['pages'] == document['pages']
    assert literal_layout_locations(artifacts, index, other) == {}

    modified = deepcopy(document)
    modified['pages'][0] += f'\n{heading["text"]}'
    assert literal_layout_locations(artifacts, index, modified) == {}
    modified = deepcopy(document)
    modified['line_units'][0][position] = 'grid-claimed-by-layout'
    assert literal_layout_locations(artifacts, index, modified) == {}
    modified = deepcopy(document)
    modified['pages'][0] = modified['pages'][0].replace(heading['text'], 'Sign    #  2.')
    assert literal_layout_locations(artifacts, index, modified) == {}
    modified['pages'][0] = modified['pages'][0].replace('Sign    #  2.', 'Sign#2.')
    assert literal_layout_locations(artifacts, index, modified) == {}


@pytest.mark.parametrize(('grid_bounds', 'grid_candidate'), [
    ({'left': 160, 'bottom': 0, 'right': 180, 'top': 100}, True),
    ({'left': 15, 'bottom': 0, 'right': 17, 'top': 100}, False),
    ({'left': None, 'bottom': 0, 'right': 180, 'top': 100}, False),
])
def test_unique_literal_grid_candidate_stays_pending(tmp_path, grid_bounds, grid_candidate):
    from types import SimpleNamespace
    from docreader.integrations.artifacts import ArtifactStore
    from docreader.tender.evidence.native import bind_pdf_literal
    from docreader.tender.native_requirement_flow import _hash, literal_layout_locations
    from test_pdf_literal_grid import fixture

    raw, literal, identity = fixture()
    # Bind original characters first. A frozen candidate alone is enough to
    # withhold ownership even when the current grid locator is far away.
    if grid_candidate:
        bind_pdf_literal(raw, literal)
        raw['units'][0]['locator'].update(grid_bounds)
    else:
        raw['units'][0]['locator'].update(grid_bounds)
        bind_pdf_literal(raw, literal)
    artifacts = ArtifactStore(tmp_path)
    raw['literal_source_ref'] = artifacts.put_json(literal)
    document = _FrozenLayout({'document_id': identity['document_id'], 'pages': ['AB'], 'line_units': [[None]]})
    document.bind_layout(_hash(raw))
    index = SimpleNamespace(raw=raw, document=identity)
    assert literal_layout_locations(artifacts, index, document) == {}
    assert any(unit.get('text_basis') == 'pdf_literal' and unit['text'] == 'AB' for unit in raw['units'])
