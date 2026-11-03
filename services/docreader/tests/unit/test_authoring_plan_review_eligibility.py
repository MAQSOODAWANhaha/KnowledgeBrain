"""Resuming host placeholders must retain their original review eligibility."""
from copy import deepcopy

import pytest

from docreader.llm.request import ModelWorkUnitExhausted
from docreader.tests.unit.test_authoring_plan_checkpoint import Interrupted, setup_pipeline
from docreader.tender import authoring_pipeline as pipeline


@pytest.mark.parametrize('reason', ['manual', 'no_support', 'write_exhausted'])
async def test_restored_placeholder_does_not_add_a_content_review(tmp_path, monkeypatch, reason):
    artifacts, snapshot, budget, materials, executor, sent = setup_pipeline(tmp_path, monkeypatch)
    original_organize = pipeline._organize

    async def organize(*args, **kwargs):
        outline = await original_organize(*args, **kwargs)
        if reason == 'manual':
            outline['chapters'][0]['targets'][0]['manual_fill'] = True
        return outline

    monkeypatch.setattr(pipeline, '_organize', organize)
    if reason != 'write_exhausted':
        materials = []

    class Executor:
        async def __call__(self, task, request, validate):
            if reason == 'write_exhausted' and task == 'authoring_write':
                sent.append(task)
                raise ModelWorkUnitExhausted(task)
            return await executor(task, request, validate)

        async def replay(self, task, request, validate):
            return await executor.replay(task, request, validate)

    plans, units = [], []

    async def publish_plan(reference):
        plans.append(artifacts.read_json(reference))

    async def publish_unit(unit):
        units.append(deepcopy(unit))
        raise Interrupted

    with pytest.raises(Interrupted):
        await pipeline.generate_document(artifacts, snapshot, budget, Executor(), materials=materials,
                                         on_plan=publish_plan, on_unit=publish_unit)
    assert units[0]['status'] == 'placeholder'
    assert 'authoring_review' not in sent
    before = list(sent)
    published = []

    async def republish(unit):
        published.append(deepcopy(unit))

    document_ref = await pipeline.generate_document(
        artifacts, snapshot, budget, Executor(), materials=materials,
        saved_plan=plans[0], saved_units=units, on_unit=republish)
    document = artifacts.read_json(document_ref)
    assert artifacts.read_json(document['unit_refs'][0])['status'] == 'placeholder'
    assert 'authoring_review' not in sent, (reason, before, sent)
    assert sent == before, (reason, before, sent)
    assert published == [], 'an unchanged immutable unit must not be published again'
