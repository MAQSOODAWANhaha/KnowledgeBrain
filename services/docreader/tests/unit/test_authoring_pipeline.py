"""Content and reference boundaries, independent of model wording."""
from copy import deepcopy
import json


from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelWorkUnitExhausted
from docreader.tender.authoring_pipeline import (
    bind_writing_response, plan_writing_tasks, prepare_materials, generate_document,
)
from docreader.tests.submission_responses import assemble_independent_candidates


def target(identity, kind='narrative'):
    return {'target_id': identity, 'kind': kind, 'purpose': '编制具体响应', 'rule_refs': [], 'source_refs': []}


def outline():
    return {'schema_version': 2, 'authoring_input': {'schema_version': 2, 'run_id': 'run',
            'source_version': 1, 'domain_revision': 1}, 'issues': [], 'chapters': [
        {'chapter_id': 'chapter-1', 'title': '技术文件', 'children': [], 'targets': [
            target('n1'), target('n2'), target('f1', 'form'), target('f2', 'form'), target('a1', 'attachment')]}]}


def test_logical_tasks_group_prose_but_keep_distinct_form_instances():
    tasks = plan_writing_tasks(outline())
    assert [[t['target_id'] for t in task['targets']] for task in tasks] == [
        ['n1', 'n2'], ['f1'], ['f2'], ['a1']]
    assert len({t['task_id'] for t in tasks}) == 4


def test_empty_response_is_a_placeholder_with_location():
    task = plan_writing_tasks(outline())[0]
    unit = bind_writing_response(task, {'blocks': []}, {}, [])
    assert unit['status'] == 'placeholder'
    assert unit['target_ids'] == ['n1', 'n2'] and unit['gaps']


def test_material_excerpt_does_not_become_original_attachment(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    materials, issues = prepare_materials(artifacts, [{'material_id': 'm1', 'title': '检索片段',
        'text': '某产品能力介绍', 'attachment': True}])
    assert materials[0]['content_ref'] and materials[0]['original_ref'] is None
    assert not materials[0]['attachment'] and issues
    task = plan_writing_tasks(outline())[-1]
    unit = bind_writing_response(task, {'blocks': [], 'attachment_ids': ['k1']}, {}, materials)
    assert unit['attachments'] == [] and unit['status'] == 'placeholder'


def test_unknown_source_fill_preserves_text_as_a_located_gap():
    task = plan_writing_tasks(outline())[1]
    unit = bind_writing_response(task, {'blocks': [], 'cell_fills': [
        {'source_id': 'wrong', 'row': 0, 'column': 1, 'text': '实际给定名称'}]}, {}, [])
    assert not unit['cell_fills'] and '实际给定名称' in ''.join(unit['gaps'])


async def test_missing_analysis_and_failed_unit_still_produce_a_real_document(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    budget = RequestBudget(model='fixture', context_tokens=131072, prompts={}, output_reserve=16384)
    calls = []

    async def execute(task, request, validate):
        calls.append(task)
        if task == 'outline':
            payload = json.loads(request['messages'][1]['content'][0]['text'])
            if payload.get('operation') == 'assemble':
                return validate(assemble_independent_candidates(payload))
            return validate({'schema_version': 2, 'chapters': [
                {'title': '技术方案', 'targets': [{'kind': 'narrative', 'purpose': '写本项目方法',
                  'rule_ids': [], 'range_ids': []}], 'children': []}], 'findings': []})
        if task == 'authoring_basis':
            return validate({'approach': '拟采用分阶段实施方式'})
        if task == 'authoring_write':
            raise ModelWorkUnitExhausted(task)
        return validate({'findings': []})

    snapshot = deepcopy(outline()['authoring_input'])
    result_ref = await generate_document(artifacts, snapshot, budget, execute, materials=[
        {'material_id': 'implementation', 'title': '给定实施方法', 'text': '按部署、测试、验收阶段实施。'}])
    result = artifacts.read_json(result_ref)
    assert result['docx_ref'] and result['package_ref']
    unit = artifacts.read_json(result['unit_refs'][0])
    assert unit['status'] == 'placeholder' and unit['error_type'] == 'ModelWorkUnitExhausted'
    assert result['gaps'] and calls.count('authoring_write') == 1


async def test_cross_batch_assembly_merges_targets_and_preserves_manual_instance(monkeypatch, tmp_path):
    from types import SimpleNamespace
    import docreader.tender.authoring_pipeline as pipeline
    from docreader.tender.evidence.references import EvidenceRepository

    artifacts = ArtifactStore(tmp_path)
    source_ref = {'source_index_ref': artifacts.put_json({'synthetic': True}), 'unit_id': 'table'}
    snapshot = deepcopy(outline()['authoring_input'])
    source_rows = [{'source': {'range_id': key, 'text': '甲' * 3000}, 'rules': []} for key in ('a', 'b')]
    monkeypatch.setattr(pipeline, '_source_records', lambda artifacts, context: source_rows)

    def bind(value):
        target = value['chapters'][0]['targets'][0]
        return {'schema_version': 2, 'authoring_input': snapshot, 'issues': [], 'chapters': [
            {'chapter_id': 'chapter-1', 'title': '报价', 'children': [], 'targets': [
                {'target_id': 'chapter-1:target-1', 'kind': 'form', 'purpose': target['purpose'],
                 'rule_refs': [], 'source_refs': [source_ref], 'manual_fill': 'b' in target['range_ids']}]}]}

    context = SimpleNamespace(input={**snapshot, 'issues': []}, payload={'rules': [], 'relations': [], 'issues': []},
                              sources={'a': [source_ref], 'b': [source_ref]}, rules={}, validate=bind,
                              repository=EvidenceRepository(artifacts))
    calls = []
    async def execute(task, request, validate):
        payload = json.loads(request['messages'][1]['content'][0]['text'])
        calls.append(payload)
        if payload.get('operation') == 'assemble':
            refs = [t['target_id'] for chapter in payload['chapters'] for t in chapter['targets']]
            assert refs == ['t1', 't2']
            return validate({'chapters': [{'title': '报价文件', 'is_volume': True, 'targets': [
                {'kind': 'form', 'purpose': '同一报价表', 'target_ids': refs,
                 'template_part_ids': []}], 'children': []}]})
        return validate({'schema_version': 2, 'chapters': [{'title': '报价', 'targets': [
            {'kind': 'form', 'purpose': '同一报价表', 'rule_ids': [],
             'range_ids': [payload['sources'][0]['range_id']]}], 'children': []}], 'findings': []})

    budget = RequestBudget(model='fixture', context_tokens=24000, prompts={}, output_reserve=4000)
    result = await pipeline._organize(artifacts, context, budget, execute, [])
    targets = result['chapters'][0]['targets']
    assert len(calls) == 3 and len(targets) == 1
    assert targets[0]['manual_fill'] and targets[0]['source_refs'] == [source_ref]
    assert result['chapters'][0]['is_volume']


async def test_absent_scoped_proof_keeps_position_without_a_model_call(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    other_original = artifacts.put_bytes(b'synthetic other-lot original', 'text/plain')
    materials = [{'material_id': 'other-proof', 'title': '另一标段证明', 'text': '仅供另一范围使用',
                  'original_ref': other_original, 'usage': 'proof', 'attachment': True,
                  'locator': {'target_ids': ['different-target']}},
                 {'material_id': 'technical', 'title': '技术方法', 'text': '技术方法仍正常生成。',
                  'locator': {'target_ids': ['chapter-1:target-1']}}]
    writes = []
    async def execute(task, request, validate):
        payload = json.loads(request['messages'][1]['content'][0]['text'])
        if task == 'outline':
            if payload.get('operation') == 'assemble':
                return validate(assemble_independent_candidates(payload))
            return validate({'schema_version': 2, 'chapters': [
                {'title': '技术方案', 'children': [], 'targets': [
                    {'kind': 'narrative', 'purpose': '具体实施方法', 'rule_ids': [], 'range_ids': []}]},
                {'title': '资格证明', 'children': [], 'targets': [
                    {'kind': 'attachment', 'purpose': '本标段资格原件', 'rule_ids': [], 'range_ids': []}]}],
                'findings': []})
        if task == 'authoring_basis':
            return validate({'approach': '拟议实施方案'})
        if task == 'authoring_write':
            writes.append(payload)
            assert payload['targets'][0]['kind'] == 'narrative'
            return validate({'blocks': [{'kind': 'paragraph', 'text': '技术方法仍正常生成。',
                'target_ids': [payload['targets'][0]['target_id']], 'support_ids': ['k1']}]})
        return validate({'findings': []})

    result = artifacts.read_json(await generate_document(artifacts, outline()['authoring_input'],
        RequestBudget(model='fixture', context_tokens=131072, output_reserve=32768), execute, materials=materials))
    units = [artifacts.read_json(ref) for ref in result['unit_refs']]
    assert len(writes) == 1 and units[0]['status'] == 'generated'
    assert units[1]['status'] == 'placeholder' and units[1]['attachments'] == []
    assert units[1]['error_type'] is None and '原件' in ''.join(units[1]['gaps'])
