"""Only the adopted assembly should describe the current assembly gap."""

import pytest

from docreader.llm.request import ModelWorkUnitExhausted
from test_assembly_candidate_scope import organize, split


def unavailable(_value):
    raise ModelWorkUnitExhausted('fixture exhausted')


@pytest.mark.parametrize('initial', [unavailable, lambda value: split(value, positions=1)])
async def test_complete_revision_replaces_the_initial_assembly_gap(tmp_path, initial):
    _, _, outline, _, gaps = await organize(tmp_path, initial, review=True, revision=split)
    assert [chapter['title'] for chapter in outline['chapters']] == ['业务范围1', '业务范围2']
    assert not any(gap['code'].startswith('outline_assembly_') for gap in gaps)


async def test_incomplete_revision_retains_only_its_current_gap(tmp_path):
    _, _, outline, _, gaps = await organize(tmp_path, unavailable, review=True,
        revision=lambda value: split(value, positions=1))
    assert outline['chapters'][-1]['title'] == '待整理内容'
    assert [gap['code'] for gap in gaps] == ['outline_assembly_incomplete']


async def test_failed_revision_preserves_the_initial_assembly_failure(tmp_path):
    _, _, outline, _, gaps = await organize(tmp_path, unavailable, review=True, revision=unavailable)
    assert outline['chapters'][0]['title'] == '原复合范围'
    assert [gap['code'] for gap in gaps] == ['outline_assembly_unavailable']


async def test_successful_revision_preserves_gaps_from_other_work(tmp_path):
    previous = {'code': 'outline_assembly_unavailable', 'detail': '另一份工作的待核记录。'}
    _, _, _, _, gaps = await organize(tmp_path, unavailable, review=True, revision=split,
                                      initial_gaps=[previous])
    assert gaps == [previous]
