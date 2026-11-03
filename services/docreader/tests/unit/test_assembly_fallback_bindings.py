"""A failed assembly retains host candidate identity through structure repair."""

from copy import deepcopy
import json

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelWorkUnitExhausted
from docreader.tender import authoring_pipeline as pipeline
from test_authoring_templates import create_context, create_source


def targets(chapters):
    return [target for chapter in pipeline._flatten(chapters) for target in chapter['targets']]


async def organize(tmp_path, *, check_bindings=False, explicit_templates=False):
    artifacts, index_ref, native = create_source(tmp_path, [
        {'text': 'Repeated fixed source title'}, {'text': 'First complete declaration and signature'},
        {'text': 'Repeated fixed source title'}, {'text': 'Second complete declaration and signature'}])
    context = create_context(artifacts, index_ref, native)
    before = deepcopy((context.payload, native.raw, artifacts.read_json(index_ref)))
    original_sources = deepcopy(context.sources)
    calls, assembly_calls, review_calls, originals = [], [], [], {}

    async def execute(stage, request, validate):
        value = json.loads(request['messages'][1]['content'][0]['text'])
        calls.append(stage)
        if stage == 'authoring_structure':
            review_calls.append(deepcopy(value))
            if len(review_calls) > 1:
                return validate({'findings': []})
            first = next(target for chapter in pipeline._flatten(value['chapters'])
                         if chapter.get('presentation_role', 'body') == 'body'
                         for target in chapter['targets'])
            assert first['target_id'] == 'v2'
            return validate({'findings': [{'code': 'check_order', 'detail': 'Retain each actual instance.'}],
                'source_decoration_exclusions': [{'target_id': first['target_id'],
                    'support_ids': first['template_support_ids'][:1],
                    'reason': 'This exact standalone part is a fixed source title.'}]})
        if value.get('operation') == 'assemble':
            assembly_calls.append(deepcopy(value))
            originals.update({target['target_id']: target for chapter in value['chapters']
                              for target in chapter['targets']})
            if 'previous_structure' not in value:
                raise ModelWorkUnitExhausted('A0 transport attempts exhausted')
            previous = targets(value['previous_structure'])
            assert len(previous) == 4
            if check_bindings:
                assert [(target['review_target_id'], target['candidate_target_ids']) for target in previous] == [
                    ('v1', ['t1']), ('v2', ['t2']), ('v3', ['t3']), ('v4', ['t4'])]
            assert previous[1]['template_part_ids'] == originals['t2']['template_part_ids'][1:]
            assert previous[3]['template_part_ids'] == originals['t4']['template_part_ids']

            def select(identity):
                candidate = originals[identity]
                result = {'kind': candidate['kind'], 'purpose': candidate['purpose'],
                          'target_ids': [identity]}
                if explicit_templates and identity == 't2':
                    result['template_part_ids'] = candidate['template_part_ids']
                return result

            # Reordering tests identity binding instead of positional inheritance.
            return validate({'chapters': [
                {'title': 'Retained body', 'targets': [select('t3'), select('t2')]},
                {'title': 'Retained submission notes', 'presentation_role': 'submission_note',
                 'targets': [select('t4'), select('t1')]}]})
        parts = {part['range_ids'][0]: part['part_id'] for part in value['template_parts']}
        ranges = [entry['range_id'] for entry in value['sources']]

        def declaration(indices):
            return {'kind': 'form', 'purpose': 'Complete this declaration',
                    'rule_ids': [],
                    'range_ids': [ranges[index] for index in indices],
                    'template_part_ids': [parts[ranges[index]] for index in indices]}

        # Candidate order is deliberately interleaved across both roles, with
        # identical titles and purposes but distinct frozen source references.
        return validate({'schema_version': 2, 'findings': [], 'chapters': [
            {'title': 'Declaration', 'presentation_role': 'submission_note', 'targets': [declaration([0, 1])]},
            {'title': 'Declaration', 'targets': [declaration([2, 3]), declaration([0, 1])]},
            {'title': 'Declaration', 'presentation_role': 'submission_note', 'targets': [declaration([2, 3])]}]})

    gaps = []
    result = await pipeline._organize(artifacts, context,
        RequestBudget(model='fixture', context_tokens=131072), execute, gaps, structure_results={})
    assert (context.payload, native.raw, artifacts.read_json(index_ref)) == before
    assert all(context.sources[identity] == refs for identity, refs in original_sources.items())
    assert calls == ['outline', 'outline', 'authoring_structure', 'outline', 'authoring_structure']
    assert not gaps
    return result, assembly_calls


async def test_fallback_previous_structure_keeps_exact_candidates_for_both_roles(tmp_path):
    result, calls = await organize(tmp_path, check_bindings=True)
    assert len(calls) == 2
    assert [chapter['presentation_role'] for chapter in result['chapters']] == ['body', 'submission_note']


@pytest.mark.parametrize('explicit_templates', [False, True])
async def test_fallback_revision_keeps_f0_exclusions_with_implicit_or_explicit_templates(
        tmp_path, explicit_templates):
    result, _ = await organize(tmp_path, explicit_templates=explicit_templates)
    first, second, shared_instance, _ = targets(result['chapters'])
    assert {part['source_ref']['unit_id'] for part in first['template_refs']} == {'unit:0', 'unit:1'}
    assert {part['source_ref']['unit_id'] for part in second['template_refs']} == {'unit:3'}
    assert {part['source_ref']['unit_id'] for part in shared_instance['template_refs']} == {'unit:2', 'unit:3'}
    assert {ref['unit_id'] for ref in second['source_refs']} == {'unit:2', 'unit:3'}
