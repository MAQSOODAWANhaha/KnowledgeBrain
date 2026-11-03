"""Use the real authoring pipeline to preserve selected support and recovery inputs."""
from copy import deepcopy
from io import BytesIO
import json
from zipfile import ZipFile

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.store import Conflict
from docreader.llm.budget import RequestBudget
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.schema import AuthoringInput


def setup_pipeline(tmp_path, monkeypatch):
    artifacts = ArtifactStore(tmp_path)
    snapshot = AuthoringInput(schema_version=2, run_id='fixture', source_version=1, domain_revision=1).model_dump()
    outline = {'schema_version': 2, 'authoring_input': snapshot, 'issues': [], 'chapters': [
        {'chapter_id': 'chapter', 'title': '实施方案', 'children': [], 'targets': [
            {'target_id': 'target', 'kind': 'narrative', 'purpose': '有据编制实施方案',
             'rule_refs': [], 'source_refs': []}]}]}
    async def organize(*args, **kwargs):
        return deepcopy(outline)
    monkeypatch.setattr(pipeline, '_organize', organize)
    original_fits = pipeline._fits
    def fits(budget, task, payload, **kwargs):
        if task == 'authoring_write':
            return len([s for s in payload['supports'] if s['origin'] == 'knowledge']) <= 2
        return original_fits(budget, task, payload, **kwargs)
    monkeypatch.setattr(pipeline, '_fits', fits)
    materials = [{'material_id': identity, 'title': identity, 'text': '实施依据' + identity,
                  'locator': {'retrievals': [{'query_id': query, 'rank': rank}]}}
                 for identity, query, rank in [('m1', 'q1', 1), ('m2', 'q1', 2), ('m3', 'q2', 1)]]
    budget = RequestBudget(model='fixture', context_tokens=131072, prompts={})
    sent, raw = [], {}
    class Executor:
        async def __call__(self, task, request, validate):
            sent.append(task)
            payload = json.loads(request['messages'][1]['content'][0]['text'])
            value = {'findings': []}
            if task == 'authoring_basis':
                value = {'shared_arrangements': []}
            elif task == 'authoring_write':
                assert [s['title'] for s in payload['supports'] if s['origin'] == 'knowledge'] == ['m1', 'm3']
                value = {'blocks': [{'kind': 'paragraph', 'text': '按m3实施',
                                      'target_ids': ['t1'], 'support_ids': ['k2']}]}
            raw[json.dumps(request, sort_keys=True)] = value
            return validate(value)
        async def replay(self, task, request, validate):
            key = json.dumps(request, sort_keys=True)
            return (True, validate(deepcopy(raw[key]))) if key in raw else (False, None)
    return artifacts, snapshot, budget, materials, Executor(), sent


async def test_crop_uses_same_k2_in_payload_adoption_and_review(tmp_path, monkeypatch):
    artifacts, snapshot, budget, materials, execute, sent = setup_pipeline(tmp_path, monkeypatch)
    result = artifacts.read_json(await pipeline.generate_document(
        artifacts, snapshot, budget, execute, materials=materials))
    unit = artifacts.read_json(result['unit_refs'][0])
    assert unit['blocks'][0]['material_ids'] == ['m3']
    assert unit['input_digest'] and sent.count('authoring_write') == 1


async def test_cropped_k2_keeps_the_same_proof_through_review_revision_and_export(tmp_path, monkeypatch):
    artifacts, snapshot, budget, materials, _, _ = setup_pipeline(tmp_path, monkeypatch)
    originals = {identity: artifacts.put_bytes((identity + ' original proof').encode(), 'text/plain')
                 for identity in ('m2', 'm3')}
    for material in materials:
        if material['material_id'] in originals:
            material.update(attachment=True, original_ref=originals[material['material_id']])
    calls = []

    async def execute(task, request, validate):
        payload = json.loads(request['messages'][1]['content'][0]['text'])
        calls.append((task, payload))
        if task == 'authoring_basis':
            return validate({'shared_arrangements': []})
        view = payload['units'][0] if task == 'authoring_review' else payload
        assert [(support['support_id'], support['title']) for support in view['supports']
                if support['origin'] == 'knowledge'] == [('k1', 'm1'), ('k2', 'm3')]
        if task == 'authoring_write':
            revised = 'previous_content' in payload
            if revised:
                previous = payload['previous_content']
                assert previous['blocks'][0]['support_ids'] == ['k2']
                assert previous['attachments'] == [
                    {'content_id': 'a1', 'attachment_id': 'k2', 'title': 'm3'}]
            return validate({'blocks': [{'kind': 'paragraph',
                'text': '按m3细化实施' if revised else '按m3实施',
                'target_ids': ['t1'], 'support_ids': ['k2']}], 'attachment_ids': ['k2']})
        assert task == 'authoring_review'
        assert view['content']['blocks'][0]['support_ids'] == ['k2']
        assert view['content']['attachments'] == [
            {'content_id': 'a1', 'attachment_id': 'k2', 'title': 'm3'}]
        if sum(stage == 'authoring_review' for stage, _ in calls) == 1:
            return validate({'findings': [{'code': 'response_imprecise', 'detail': '依据现有资料细化步骤。',
                'task_ids': [view['task_id']], 'content_ids': ['b1'], 'can_revise': True}]})
        assert view['content']['blocks'][0]['text'] == '按m3细化实施'
        return validate({'findings': []})

    result = artifacts.read_json(await pipeline.generate_document(
        artifacts, snapshot, budget, execute, materials=materials))
    assert [task for task, _ in calls if task != 'authoring_basis'] == [
        'authoring_write', 'authoring_review', 'authoring_write', 'authoring_review']
    units = [artifacts.read_json(reference) for reference in
             [*result['superseded_unit_refs'], *result['unit_refs']]]
    for unit in units:
        assert unit['blocks'][0]['material_ids'] == ['m3']
        assert unit['attachments'] == [
            {'material_id': 'm3', 'title': 'm3', 'artifact_ref': originals['m3']}]
    assert units[-1]['blocks'][0]['text'] == '按m3细化实施'
    with ZipFile(BytesIO(artifacts.read_bytes(result['package_ref']))) as archive:
        attachment, = json.loads(archive.read('manifest.json'))['attachments']
        assert attachment['material_id'] == 'm3' and attachment['artifact_ref'] == originals['m3']
        assert archive.read(attachment['package_path']) == b'm3 original proof'


def test_explicit_empty_selection_keeps_no_candidate(tmp_path, monkeypatch):
    _, _, _, materials, _, _ = setup_pipeline(tmp_path, monkeypatch)
    task = {'targets': [{'target_id': 'target'}], '_selected_material_ids': []}
    assert pipeline._task_materials(materials, task) == []
    task.pop('_selected_material_ids')
    assert [m['material_id'] for m in pipeline._task_materials(materials, task)] == ['m1', 'm2', 'm3']


class Interrupted(Exception):
    pass


async def test_committed_plan_restores_selection_before_any_write(tmp_path, monkeypatch):
    artifacts, snapshot, budget, materials, execute, sent = setup_pipeline(tmp_path, monkeypatch)
    plans = []
    async def publish(reference):
        plans.append(artifacts.read_json(reference))
        raise Interrupted
    with pytest.raises(Interrupted):
        await pipeline.generate_document(artifacts, snapshot, budget, execute,
                                         materials=materials, on_plan=publish)
    assert 'authoring_write' not in sent
    assert plans[0]['tasks'][0]['_selected_material_ids'] == ['m1', 'm3']
    stored = artifacts.read_json(plans[0]['materials_ref'])
    assert [m['material_id'] for m in stored] == ['m1', 'm2', 'm3']
    original = pipeline._fits
    def no_refit(budget, task, payload, **kwargs):
        assert task != 'authoring_write', 'a committed plan must skip fit_task'
        return original(budget, task, payload, **kwargs)
    monkeypatch.setattr(pipeline, '_fits', no_refit)
    result = artifacts.read_json(await pipeline.generate_document(
        artifacts, snapshot, budget, execute, materials=materials, saved_plan=plans[0]))
    assert artifacts.read_json(result['unit_refs'][0])['blocks'][0]['material_ids'] == ['m3']
    assert any(g['code'] == 'knowledge_candidates_not_loaded' for g in result['gaps'])


async def test_old_saved_unit_is_rebound_from_identical_raw_response(tmp_path, monkeypatch):
    artifacts, snapshot, budget, materials, execute, sent = setup_pipeline(tmp_path, monkeypatch)
    plans, units = [], []
    async def publish_plan(reference):
        plans.append(artifacts.read_json(reference))
    async def publish_unit(unit):
        units.append(deepcopy(unit))
        raise Interrupted
    with pytest.raises(Interrupted):
        await pipeline.generate_document(artifacts, snapshot, budget, execute, materials=materials,
                                         on_plan=publish_plan, on_unit=publish_unit)
    old = deepcopy(units[0])
    old.pop('input_digest', None)
    old['blocks'][0]['material_ids'] = ['m2']
    result = artifacts.read_json(await pipeline.generate_document(
        artifacts, snapshot, budget, execute, materials=materials, saved_plan=plans[0], saved_units=[old]))
    adopted = artifacts.read_json(result['unit_refs'][0])
    assert adopted['blocks'][0]['material_ids'] == ['m3']
    assert adopted['input_digest'] and sent.count('authoring_write') == 1


async def test_saved_plan_is_checked_against_actual_dependencies(tmp_path, monkeypatch):
    artifacts, snapshot, budget, materials, execute, sent = setup_pipeline(tmp_path, monkeypatch)
    plans = []
    async def publish(reference):
        plans.append(artifacts.read_json(reference))
        raise Interrupted
    with pytest.raises(Interrupted):
        await pipeline.generate_document(artifacts, snapshot, budget, execute, materials=materials, on_plan=publish)
    # Its self-declared digest is unchanged, but the actual input changed.
    changed = deepcopy(materials)
    changed[2]['text'] = '另一个实际版本的实施依据'
    with pytest.raises(Conflict):
        await pipeline.generate_document(artifacts, snapshot, budget, execute,
                                         materials=changed, saved_plan=plans[0])
    assert 'authoring_write' not in sent
