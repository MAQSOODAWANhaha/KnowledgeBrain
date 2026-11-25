"""Final reviewed-source receipts refer to the saved outline, after target binding."""
from typing import Any, cast

from copy import deepcopy

import pytest

from docreader.llm.budget import RequestBudget
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender import reviewed_source_facts as reviewed
from test_directory_compile import item, node, plan
from test_reviewed_source_facts import fixture, point, window


@pytest.mark.parametrize('empty,fail_receipt,usable', [
    (False, False, True), (True, False, True), (False, True, True), (False, False, False)])
async def test_final_fact_receipt_is_bound_to_saved_outline_and_precedes_publication(
        tmp_path, monkeypatch, empty, fail_receipt, usable):
    artifacts, snapshot, documents, manifest, _, _ = fixture(tmp_path)
    if empty:
        manifest['facts'] = []
    manifest_ref = artifacts.put_json(manifest)
    facts = reviewed.load_reviewed_facts(artifacts, snapshot, documents, manifest_ref)
    raw_plan = plan([
        {**node('financial', 1, 0), 'heading': point(1), 'body_windows': [window(1, 0, 2)]},
        {**node('solution', 2, 0), 'heading': point(2), 'body_windows': [window(2, 0, 1)],
         'response_kind': 'narrative'},
    ], [item('financial'), item('solution')])
    observed = []

    async def plans(*args, **kwargs):
        return ([raw_plan] if usable else []), []

    async def details(compiled, *_args, **_options):
        reviewed.apply_reviewed_material_facts(compiled, documents, facts)
        owner = next(c['id'] for c in compiled['chapters'] if c['response_kind'] == 'narrative')
        reviewed.apply_reviewed_body_dispositions(compiled, documents, [
            {'fact_id': f['fact_id'], 'scope': 'chapter', 'chapter_id': owner}
            for f in facts if f['kind'] == 'independent_body_response'], facts)
        return compiled

    async def no_model(*args, **kwargs):
        pytest.fail('Final host receipt persistence does not require a model call.')

    async def save_facts(outline_ref, records):
        outline = artifacts.read_json(outline_ref)
        chapters = {c['chapter_id']: c for c in pipeline._flatten(outline['chapters'])}
        targets = {t['target_id']: t for c in chapters.values() for t in c['targets']}
        assert {r['fact_id'] for r in records} == {f['fact_id'] for f in facts}
        assert all(r['enterprise_confirmed'] is False for r in records)
        for record in records:
            if usable:
                assert record['final_chapter_ids'] and set(record['final_chapter_ids']) <= chapters.keys()
                assert record['target_ids'] and set(record['target_ids']) <= targets.keys()
            else:
                assert record['state'] == 'unresolved'
                assert record['final_chapter_ids'] == [] and record['target_ids'] == []
        if records and usable:
            body = next(r for r in records if r['kind'] == 'independent_body_response')
            assert len(body['target_ids']) == 1
            assert targets[body['target_ids'][0]]['purpose'] == facts[-1]['_source_text']
        observed.append(('facts', outline_ref, deepcopy(records)))
        if fail_receipt:
            raise RuntimeError('reviewed receipt persistence failed')

    async def publish(outline_ref):
        observed.append(('outline', outline_ref))

    monkeypatch.setattr(pipeline, '_request_directory_plans', plans)
    monkeypatch.setattr(pipeline, '_request_directory_details', details)
    options = dict(outline_only=True, directory_documents=documents,
        source_carrier_contract='source_carriers_closed_v1', reviewed_source_facts_ref=manifest_ref,
        on_reviewed_facts=save_facts, on_outline=publish)
    budget = RequestBudget(model='synthetic', context_tokens=131072, output_reserve=32768)
    if fail_receipt:
        with pytest.raises(RuntimeError, match='receipt persistence failed'):
            await pipeline.generate_document(artifacts, snapshot, budget, no_model, **cast(Any, options))
        assert [event[0] for event in observed] == ['facts']
    else:
        result = await pipeline.generate_document(artifacts, snapshot, budget, no_model, **cast(Any, options))
        assert [event[0] for event in observed] == ['facts', 'outline']
        assert all(event[1] == result for event in observed)
