"""Reviewed source additions retain the accepted outline and its owned references."""

from copy import deepcopy

import pytest

from docreader.tender.outline_edit import apply_outline_edit
from test_outline_edit import MANUAL, outline


def source(base, unit_id='condition'):
    result = deepcopy(base['chapters'][0]['targets'][0]['source_refs'][0])
    result['unit_id'] = unit_id
    return result


def tree(base, additions):
    return [{'chapter_id': chapter['chapter_id'], 'title': chapter['title'], 'children': [],
             **({'source_additions': additions} if number == 0 else {})}
            for number, chapter in enumerate(base['chapters'])]


@pytest.mark.parametrize('location', ['submission', 'base_target', 'catalog'])
def test_addition_uses_only_owned_full_sources_and_preserves_every_other_field(location):
    base = outline()
    extra = source(base)
    catalog = []
    if location == 'submission':
        base['submission_refs'] = [extra]
    else:
        owner = {**deepcopy(base['chapters'][0]['targets'][0]), 'target_id': 'owner', 'source_refs': [extra]}
        if location == 'base_target':
            base['chapters'][1]['targets'] = [owner]
        else:
            catalog = [owner]
    before = deepcopy((base, catalog))
    existing = base['chapters'][0]['targets'][0]['source_refs'][0]
    edit = tree(base, [{'target_id': 'form-a', 'source_refs': [extra, existing, extra]},
                       {'target_id': 'form-a', 'source_refs': [extra]}])
    expected = deepcopy(base)
    expected['chapters'][0]['targets'][0]['source_refs'].append(extra)
    changed = apply_outline_edit(base, edit, target_catalog=catalog)
    assert changed == expected
    assert (base, catalog) == before
    assert apply_outline_edit(changed, edit, target_catalog=catalog) == changed


def test_addition_can_follow_a_moved_or_restored_target_into_its_selected_chapter():
    base = outline()
    extra = source(base)
    base['submission_refs'] = [extra]
    removed = apply_outline_edit(base, [{'chapter_id': 'b', 'title': '恢复位置', 'children': []}])
    changed = apply_outline_edit(removed, [{'chapter_id': 'b', 'title': '恢复位置', 'children': [],
        'target_ids': ['form-a'], 'source_additions': [{'target_id': 'form-a', 'source_refs': [extra]}]}],
        target_catalog=base['chapters'][0]['targets'])
    expected = deepcopy(base['chapters'][0]['targets'][0])
    expected['source_refs'].append(extra)
    assert changed['chapters'][0]['targets'] == [expected]
    assert changed['issues'] == []


@pytest.mark.parametrize('wrong', ['unknown_unit', 'other_artifact', 'changed_size', 'other_chapter', 'unknown_target'])
def test_addition_rejects_unknown_identity_and_cross_chapter_target(wrong):
    base = outline()
    extra = source(base)
    base['submission_refs'] = [deepcopy(extra)]
    target_id = 'form-a'
    if wrong == 'unknown_unit':
        extra['unit_id'] = 'not-read'
    elif wrong == 'other_artifact':
        extra['source_index_ref'].update(sha256='b' * 64, relative_path='bb/' + 'b' * 64)
    elif wrong == 'changed_size':
        extra['source_index_ref']['size'] += 1
    elif wrong == 'unknown_target':
        target_id = 'unknown-target'
    else:
        base['chapters'][1]['targets'] = base['chapters'][0]['targets']
        base['chapters'][0]['targets'] = []
    with pytest.raises(ValueError, match='来源|响应位置'):
        apply_outline_edit(base, tree(base, [{'target_id': target_id, 'source_refs': [extra]}]))


def test_addition_cannot_create_a_target_or_overwrite_owned_target_fields():
    base = outline()
    extra = source(base)
    base['submission_refs'] = [extra]
    with pytest.raises(ValueError, match='响应位置'):
        apply_outline_edit(base, [{'chapter_id': MANUAL, 'title': '新章节', 'writing_goal': '人工目标',
            'source_additions': [{'target_id': 'manual-response-' + MANUAL, 'source_refs': [extra]}]}])
    for field, value in [('rule_refs', []), ('template_refs', []), ('support_origins', ['tender']),
                         ('purpose', '替换用途'), ('manual_fill', False)]:
        with pytest.raises(ValueError):
            apply_outline_edit(base, tree(base, [{'target_id': 'form-a', 'source_refs': [extra], field: value}]))


def test_empty_addition_preserves_existing_behavior():
    base = outline()
    assert apply_outline_edit(base, tree(base, [])) == base


async def test_reviewed_addition_reaches_only_the_changed_writing_task(tmp_path):
    from docreader.tender import authoring_pipeline as pipeline
    from test_assembly_source_handoff import CONDITION, UNRELATED, organize

    artifacts, context, base, _ = await organize(tmp_path)
    other = deepcopy(base['chapters'][0])
    other['chapter_id'] = 'other-chapter'
    other['targets'][0]['target_id'] = 'other-target'
    base['chapters'].append(other)
    original = deepcopy(base)
    condition = context.sources[context.payload['rules'][2]['item']['evidence'][0]['range_id']][0]
    assert condition in base['submission_refs']
    target = base['chapters'][0]['targets'][0]
    edited = apply_outline_edit(base, tree(base, [{'target_id': target['target_id'], 'source_refs': [condition]}]))
    before_task = pipeline.plan_writing_tasks(base)[0]
    after_task = pipeline.plan_writing_tasks(edited)[0]
    basis = {'response': {'shared_arrangements': []}}
    before, _, _ = pipeline._load_task(artifacts, context, before_task, basis, [])
    after, _, issues = pipeline._load_task(artifacts, context, after_task, basis, [])
    assert CONDITION not in [entry['text'] for entry in before['supports']]
    texts = [entry['text'] for entry in after['supports']]
    assert texts.count(CONDITION) == 1 and UNRELATED not in texts and not issues
    assert len(after_task['targets'][0]['rule_refs']) == 2
    assert after_task['targets'][0]['template_refs'] == []
    assert after_task['targets'][0]['support_origins'] == ['tender']
    assert pipeline.plan_writing_tasks(edited)[1] == pipeline.plan_writing_tasks(base)[1]
    assert base == original
