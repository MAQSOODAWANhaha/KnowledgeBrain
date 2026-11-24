"""New-chain proposals gain a writing position while their original forms stay intact."""

from copy import deepcopy
from io import BytesIO
import json

from docx import Document
import pytest

from docreader.llm.budget import RequestBudget
from docreader.tender import authoring_coverage
from docreader.tender.authoring_excerpts import load_source_excerpt
from docreader.tender.authoring_pipeline import generate_document
from docreader.tender.bid_format_outline import BID_FORMAT_OUTLINE_CONTRACT
from docreader.tender.item_placement import REQUIREMENT_CHAIN_CONTRACT
from docreader.tender.outline_edit import apply_outline_edit

from test_outline_organization import context


_INSTRUCTION = '投标人应根据“第三章 评标办法”、“第五章 采购需求”等要求编制相关内容。'
_ARTIFACT = {
    'sha256': '0' * 64,
    'relative_path': '00/' + '0' * 64,
    'size': 1,
    'media_type': 'application/json',
}
_RULE_REF = {'item_ref': {'artifact_ref': _ARTIFACT, 'item_id': 'service-plan'}}
_GRID = {'source_ref': {'source_index_ref': _ARTIFACT, 'unit_id': 'page:110:table:0'}}


def _outline(title='项目服务方案', instruction=_INSTRUCTION):
    original = {
        'target_id': 'chapter-1:target-1',
        'kind': 'form',
        'purpose': f'十一、{title}\n{instruction}',
        'rule_refs': [deepcopy(_RULE_REF)],
        'source_refs': [],
        'template_refs': [],
        'manual_fill': True,
        'support_origins': ['tender'],
    }
    return {'issues': [], 'chapters': [{'chapter_id': 'chapter-1', 'title': title,
            'targets': [original], 'children': [], 'presentation_role': 'body'}]}


def test_w2_proposal_retains_its_original_and_moves_assigned_rules_to_one_knowledge_answer():
    outline = _outline()
    original = deepcopy(outline['chapters'][0]['targets'][0])

    authoring_coverage.attach_w2_proposal_answers(outline)
    authoring_coverage.attach_w2_proposal_answers(outline)

    form, answer = outline['chapters'][0]['targets']
    assert form == {**original, 'rule_refs': []}
    assert answer['target_id'] == original['target_id'] + ':answer'
    assert answer['kind'] == 'narrative'
    assert answer['manual_fill'] is False
    assert answer['rule_refs'] == original['rule_refs']
    assert answer['related_target_ids'] == [original['target_id']]
    assert answer['support_origins'] == ['knowledge']
    assert answer['source_refs'] == []


@pytest.mark.parametrize(('title', 'instruction', 'overrides'), [
    ('服务承诺', _INSTRUCTION, {}),
    ('项目服务方案', _INSTRUCTION, {'template_refs': [_GRID]}),
    ('项目服务方案', _INSTRUCTION + '\n投标人（盖章）：________', {}),
    ('项目服务方案', _INSTRUCTION, {'rule_refs': []}),
    ('项目服务方案', _INSTRUCTION, {'purpose': f'十一、采购需求\n{_INSTRUCTION}'}),
    ('项目服务方案', _INSTRUCTION, {'response_table': {'rows': [{'row_id': 'existing-row'}]}}),
    ('项目服务方案', '详见采购需求。', {}),
])
def test_w2_unproven_writing_positions_stay_manual(title, instruction, overrides):
    outline = _outline(title, instruction)
    outline['chapters'][0]['targets'][0].update(overrides)
    original = deepcopy(outline)

    authoring_coverage.attach_w2_proposal_answers(outline)

    assert outline == original


@pytest.mark.parametrize('material_scope', ['absent', 'another-target', 'proposal'])
async def test_w2_form_to_word_uses_only_material_scoped_to_the_proposal(tmp_path, material_scope):
    requirement = '应在巡检中按周期汇总服务记录。'
    supported = '每月汇总巡检记录并提交服务报告。'
    background = '本项目现有运维点位位于甲园区。'
    invented = '投标人在甲园区已具备自有运维团队。'
    artifacts, snapshot, source_context = await context(
        tmp_path, documents=[('main', 'main', [(requirement, 'obligation'), (background, None)])])
    heading = '一、项目服务方案'
    sources = [{'document_id': 'main', 'role': 'main',
                'pages': [f'第六章 投标文件格式\n{heading}\n{_INSTRUCTION}'],
                'line_units': [[]]}]
    outline_calls, body_calls = [], []

    async def outline_execute(stage, request, validate):
        outline_calls.append(stage)
        payload = json.loads(request['messages'][1]['content'][0]['text'])
        if stage == 'directory_plan':
            headings = {text.strip(): ref for ref, text in payload['lines']}
            return validate({'chapters': [{'heading_refs': [headings[heading]], 'name': '项目服务方案'}]})
        assert stage == 'item_placement'
        assert len(payload['positions']) == len(payload['requirements']) == 1
        assert payload['requirements'][0]['text'] == requirement
        return validate({'placements': [[payload['requirements'][0]['id'], payload['positions'][0]['id']]]})

    budget = RequestBudget(model='fixture', context_tokens=131072, output_reserve=16384)
    outline = artifacts.read_json(await generate_document(
        artifacts, snapshot, budget, outline_execute, directory_documents=sources,
        outline_only=True, directory_contract=BID_FORMAT_OUTLINE_CONTRACT,
        requirement_chain_contract=REQUIREMENT_CHAIN_CONTRACT))
    assert outline_calls == ['directory_plan', 'item_placement']
    chapter, = outline['chapters']
    form, answer = chapter['targets']
    assert not form['rule_refs'] and len(answer['rule_refs']) == 1
    index = source_context.repository.source_index(
        artifacts.read_json(snapshot['frozen_ref'])['documents'][0]['source_index_ref'])
    background_range, = [row for row in index.ranges if row['text'] == background]
    excerpts = source_context.excerpts[background_range['range_id']]
    edit = {'chapter_id': chapter['chapter_id'], 'title': chapter['title']}

    def bind(base, target, context_excerpts):
        addition = {'target_id': target['target_id'], 'project_context_excerpts': context_excerpts}
        return apply_outline_edit(base, [{**edit, 'source_additions': [addition]}], artifacts=artifacts)

    outline = bind(outline, answer, excerpts)
    saved_form, saved_answer = outline['chapters'][0]['targets']
    assert saved_form == form
    assert saved_answer['rule_refs'] == answer['rule_refs']
    assert saved_answer['project_context_excerpts'] == excerpts
    assert not saved_answer.get('source_excerpts') and not saved_answer.get('source_body_ref')
    assert apply_outline_edit(outline, [edit], artifacts=artifacts)['chapters'][0]['targets'][1] == saved_answer
    assert not bind(outline, answer, [])['chapters'][0]['targets'][1].get('project_context_excerpts')

    materials = [] if material_scope == 'absent' else [
        {'material_id': 'method', 'title': '服务规程', 'text': supported, 'locator': {
            'target_ids': [answer['target_id'] if material_scope == 'proposal' else 'other-target']}}]

    async def body_execute(stage, request, validate):
        body_calls.append(stage)
        payload = json.loads(request['messages'][1]['content'][0]['text'])
        if stage == 'authoring_basis':
            return validate({})
        if stage == 'authoring_write':
            target, = payload['targets']
            assert target['kind'] == 'narrative' and target['support_origins'] == ['knowledge']
            assert [rule['item']['text'] for rule in payload['rules']] == [requirement]
            support, = target['support_ids']
            context_id, = target['source_context_support_ids']
            by_id = {entry['support_id']: entry for entry in payload['supports']}
            assert by_id[support]['text'] == supported
            assert by_id[context_id]['origin'] == 'tender' and by_id[context_id]['text'] == background
            return validate({'blocks': [{'kind': 'paragraph', 'text': supported,
                                         'target_ids': [target['target_id']], 'support_ids': [support]},
                                        {'kind': 'paragraph', 'text': invented,
                                         'target_ids': [target['target_id']], 'support_ids': [context_id]}]})
        assert stage in ('authoring_structure', 'authoring_review')
        if stage == 'authoring_review':
            reviewed, = payload['units']
            context_id, = reviewed['targets'][0]['source_context_support_ids']
            assert next(entry for entry in reviewed['supports'] if entry['support_id'] == context_id)['text'] == background
        return validate({'findings': []})

    document = artifacts.read_json(await generate_document(
        artifacts, snapshot, budget, body_execute, outline=outline, materials=materials))
    written = material_scope == 'proposal'
    assert body_calls.count('authoring_write') == body_calls.count('authoring_review') == int(written)
    assert body_calls.count('authoring_structure') == 1
    units = [artifacts.read_json(ref) for ref in document['unit_refs']]
    original_unit, = [unit for unit in units if form['target_id'] in unit['target_ids']]
    answer_unit, = [unit for unit in units if answer['target_id'] in unit['target_ids']]
    assert original_unit['status'] == 'placeholder'
    assert answer_unit['status'] == ('generated' if written else 'placeholder')
    if written:
        block, = answer_unit['blocks']
        assert block['material_ids'] == ['method'] and not block['source_excerpts']
    assert all(gap['code'] != 'export_bound_requirement_answer_blank' for gap in document['gaps'])
    coverage = artifacts.read_json(document['coverage_ref'])
    entry, = [entry for entry in coverage['entries'] if entry['rule_ref'] is not None]
    assert entry['target_id'] == answer['target_id'] and not entry['source_excerpts']
    docx = Document(BytesIO(artifacts.read_bytes(document['docx_ref'])))
    paragraphs = '\n'.join(paragraph.text for paragraph in docx.paragraphs)
    assert _INSTRUCTION in paragraphs
    assert paragraphs.count(background) == paragraphs.count('原文上下文（非企业应答）：') == 1
    assert invented not in paragraphs
    assert (supported in paragraphs) is written
    if written:
        assert paragraphs.index(_INSTRUCTION) < paragraphs.index(background) < paragraphs.index(supported)
    else:
        assert paragraphs.index(background) < paragraphs.index('（本部分内容待补）', paragraphs.index(background))


async def test_w2_outline_edits_replace_only_same_snapshot_project_context(tmp_path):
    artifacts, snapshot, frozen = await context(tmp_path, documents=[('main', 'main', [
        ('应安排巡检。', 'obligation'), ('甲园区现有运维点位。', None), ('乙园区现有运维点位。', None)])])
    _, _, foreign = await context(tmp_path, documents=[('main', 'main', [
        ('应安排巡检。', 'obligation'), ('丙园区现有运维点位。', None)])])

    def exact_excerpts(source_context, text):
        candidates = [excerpts for excerpts in source_context.excerpts.values()
                      if load_source_excerpt(artifacts, excerpts[0])['text'] == text]
        selection, = candidates
        return selection

    outline = {**_outline(), 'schema_version': 2, 'authoring_input': snapshot}
    authoring_coverage.attach_w2_proposal_answers(outline)
    chapter, = outline['chapters']
    form, answer = chapter['targets']

    def add(base, target, excerpts):
        return apply_outline_edit(base, [{'chapter_id': chapter['chapter_id'], 'title': chapter['title'],
            'source_additions': [{'target_id': target['target_id'], 'project_context_excerpts': excerpts}]}],
            artifacts=artifacts)

    initial = exact_excerpts(frozen, '甲园区现有运维点位。')
    replacement = exact_excerpts(frozen, '乙园区现有运维点位。')
    from_other_snapshot = exact_excerpts(foreign, '丙园区现有运维点位。')
    selected = add(outline, answer, initial)
    assert add(selected, answer, replacement)['chapters'][0]['targets'][1]['project_context_excerpts'] == replacement
    with pytest.raises(ValueError, match='重叠'):
        add(selected, answer, [*initial, *initial])
    with pytest.raises(ValueError):
        add(selected, answer, from_other_snapshot)
    with pytest.raises(ValueError):
        add(selected, form, initial)
    assert selected['chapters'][0]['targets'][1]['project_context_excerpts'] == initial


@pytest.mark.parametrize('role', ['amendment', 'clarification'])
async def test_w2_outline_edit_rejects_main_background_when_supplement_scope_needs_review(tmp_path, role):
    background = '甲园区现有运维点位。'
    artifacts, snapshot, frozen = await context(tmp_path, documents=[
        ('main', 'main', [('应安排巡检。', 'obligation'), (background, None)]),
        ('supplement', role, [('现有点位范围另行说明。', None)]),
    ])
    excerpts, = [refs for refs in frozen.excerpts.values() if load_source_excerpt(artifacts, refs[0])['text'] == background]
    outline = {**_outline(), 'schema_version': 2, 'authoring_input': snapshot}
    authoring_coverage.attach_w2_proposal_answers(outline)
    chapter, = outline['chapters']
    answer = chapter['targets'][1]
    with pytest.raises(ValueError, match='补遗或澄清'):
        apply_outline_edit(outline, [{'chapter_id': chapter['chapter_id'], 'title': chapter['title'],
            'source_additions': [{'target_id': answer['target_id'], 'project_context_excerpts': excerpts}]}],
            artifacts=artifacts)
