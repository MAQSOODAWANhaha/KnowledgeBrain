"""Automatic source-outline publication rejects diagnostics without response positions."""
from copy import deepcopy

import pytest

from docreader.llm.request import ModelWorkUnitExhausted
from docreader.tender import document_execution
from docreader.tender.directory_ref_plan import DIRECTORY_REF_CONTRACT
from test_document_outline_execution import runtime as runtime


def target():
    return {'target_id': 'response', 'kind': 'form', 'purpose': '企业信息待人工填写',
            'rule_refs': [], 'source_refs': [], 'manual_fill': True, 'support_origins': ['tender']}


def chapter(role='body', *, targets=None, children=None):
    return {'chapter_id': 'chapter-1', 'title': '投标文件', 'presentation_role': role,
            'targets': targets or [], 'children': children or []}


@pytest.fixture
def source_runtime(runtime):
    runtime.snapshot['frozen_ref'] = runtime.artifacts.put_json({'documents': []})
    runtime.outline['authoring_input'] = deepcopy(runtime.snapshot)
    runtime.outline['chapters'] = []
    runtime.outline['issues'] = [{'code': 'outline_directory_plan_failed',
        'detail': '两次原件读取未完成，原始失败和未知费用仍保留。', 'rule_refs': []}]
    return runtime


@pytest.mark.parametrize('chapters', [
    [], [chapter()], [chapter('cover', targets=[target()])],
    [chapter('content', targets=[target()])],
    [chapter('submission_note', children=[{**chapter(targets=[target()]), 'chapter_id': 'chapter-1.1'}])],
])
async def test_unusable_automatic_source_outline_is_failed_and_preserved(source_runtime, chapters):
    source_runtime.outline['chapters'] = chapters
    original = deepcopy(source_runtime.outline)
    with pytest.raises(ModelWorkUnitExhausted) as caught:
        await source_runtime.execute(outline_only=True)
    assert type(caught.value).__name__ == 'SourceOutlineUnavailable'
    row = source_runtime.rows['execution']
    assert row['status'] == 'failed' and row.get('result_ref') is None
    assert row['error'] == {'type': 'SourceOutlineUnavailable'}
    store = document_execution.AuthoringStore('fixture')
    rejected = [call for call in store.commit_artifact.await_args_list
                if call.args[1] == 'directory-outline-unavailable']
    assert len(rejected) == 1
    assert source_runtime.artifacts.read_json(rejected[0].args[2]) == original
    assert rejected[0].kwargs['input_digest'] == document_execution._digest(source_runtime.snapshot)
    assert len(source_runtime.observed) == 1 and row['requests'] == []
    assert source_runtime.outline == original


async def test_nested_manual_response_is_usable_without_claiming_semantic_acceptance(source_runtime):
    child = {**chapter(targets=[target()]), 'chapter_id': 'chapter-1.1'}
    source_runtime.outline['chapters'] = [chapter(children=[child])]
    reference = await source_runtime.execute(outline_only=True)
    assert source_runtime.rows['execution']['status'] == 'succeeded'
    assert source_runtime.artifacts.read_json(reference)['issues'] == source_runtime.outline['issues']
    assert not [call for call in document_execution.AuthoringStore('fixture').commit_artifact.await_args_list
                if call.args[1] == 'directory-outline-unavailable']


async def test_diagnostic_persistence_failure_cannot_publish_empty_success(source_runtime):
    store = document_execution.AuthoringStore('fixture')

    async def commit(_lease, stage, *_args, **_kwargs):
        if stage == 'directory-outline-unavailable':
            raise OSError('synthetic diagnostic storage outage')

    store.commit_artifact.side_effect = commit
    with pytest.raises(OSError, match='diagnostic storage outage'):
        await source_runtime.execute(outline_only=True)
    assert source_runtime.rows['execution']['status'] == 'failed'
    assert source_runtime.rows['execution'].get('result_ref') is None
    assert len(source_runtime.observed) == 1


async def test_fixed_manual_empty_tree_is_unaffected(source_runtime):
    fixed = source_runtime.artifacts.put_json(source_runtime.outline)
    assert await source_runtime.execute('manual', outline_only=True, outline_ref=fixed)
    assert source_runtime.rows['manual']['status'] == 'succeeded'


async def test_missing_frozen_snapshot_cannot_publish_empty_automatic_outline(source_runtime):
    source_runtime.snapshot['frozen_ref'] = None
    source_runtime.outline['authoring_input'] = deepcopy(source_runtime.snapshot)
    with pytest.raises(document_execution.SourceOutlineUnavailable):
        await source_runtime.execute(outline_only=True)
    assert source_runtime.rows['execution']['status'] == 'failed'
    assert source_runtime.rows['execution'].get('result_ref') is None


async def test_previously_persisted_empty_success_is_not_returned_as_usable(source_runtime):
    source_runtime.outline['chapters'] = [chapter(targets=[target()])]
    await source_runtime.execute(outline_only=True)
    previous = source_runtime.rows['execution']
    empty = {**source_runtime.outline, 'chapters': []}
    previous['result_ref'] = source_runtime.artifacts.put_json(empty)
    historical = deepcopy(previous)
    with pytest.raises(ModelWorkUnitExhausted):
        await source_runtime.execute(outline_only=True)
    assert source_runtime.rows['execution'] == historical
    assert len(source_runtime.observed) == 1


@pytest.mark.parametrize('prior_contract', ['material_short_ids_v2', None])
async def test_new_material_capability_does_not_rewrite_existing_execution_identity(source_runtime, prior_contract):
    source_runtime.outline['chapters'] = [chapter(targets=[target()])]
    reference = await source_runtime.execute(outline_only=True, directory_planning_contract=DIRECTORY_REF_CONTRACT)
    spec = source_runtime.rows['execution']['execution_spec']
    assert spec['directory_material_identity_contract'] == 'material_short_ids_v3'
    if prior_contract is None:
        spec.pop('directory_material_identity_contract')
    else:
        spec['directory_material_identity_contract'] = prior_contract
    assert await source_runtime.execute(outline_only=True) == reference
    assert source_runtime.rows['execution']['execution_spec'].get('directory_material_identity_contract') == prior_contract
    assert len(source_runtime.observed) == 1
