"""Submission structure uses semantic choices, not source-table side effects."""
import json
from types import SimpleNamespace

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelWorkUnitExhausted
from docreader.tender import authoring_pipeline as pipeline


def fixture_context(monkeypatch, artifacts):
    source = {'source_index_ref': artifacts.put_json({'fixture': True}), 'unit_id': 'template'}
    snapshot = {'schema_version': 2, 'run_id': 'submission', 'source_version': 1,
                'domain_revision': 1, 'issues': []}
    context = SimpleNamespace(input=snapshot, payload={'rules': [], 'issues': [], 'relations': []},
                              sources={'source-a': [source]}, rules={},
                              template_parts={'part-a': {'source_ref': source, 'selectors': []}})

    def bind(value):
        def chapters(entries):
            return [{'chapter_id': 'local', 'title': c['title'], 'is_volume': c.get('is_volume', False),
                     'children': chapters(c['children']), 'targets': [
                         {'target_id': 'local-target', 'kind': t['kind'], 'purpose': t['purpose'],
                          'rule_refs': [], 'source_refs': [source], 'manual_fill': t.get('manual_fill', False),
                          'support_origins': ['knowledge'],
                          'template_refs': [context.template_parts[p] for p in t.get('template_part_ids', [])]}
                         for t in c['targets']]} for c in entries]
        return {'schema_version': 2, 'authoring_input': snapshot, 'issues': [],
                'chapters': chapters(value['chapters']), 'constraint_rule_refs': [], 'submission_refs': [],
                'search_intents': []}

    context.validate = bind
    rows = [{'source': {'range_id': 'source-a', 'kind': 'section', 'text': '投标函规定格式'}, 'rules': []}]
    monkeypatch.setattr(pipeline, '_source_records', lambda *_: rows)
    monkeypatch.setattr(pipeline, 'prepare_template_parts',
                        lambda *_: (rows, [{'part_id': 'part-a', 'range_ids': ['source-a']}]), raising=False)
    return context, source


def response(source_id, part_id):
    return {'schema_version': 2, 'chapters': [{'title': '局部附件格式分组', 'is_volume': True,
        'targets': [], 'children': [{'title': '投标函候选', 'children': [], 'targets': [
            {'kind': 'form', 'purpose': '填写本次投标函', 'rule_ids': [], 'range_ids': [source_id],
             'template_part_ids': [part_id]}]}]}], 'findings': []}


async def test_single_source_batch_is_not_automatically_a_final_volume(monkeypatch, tmp_path):
    artifacts = ArtifactStore(tmp_path)
    context, source = fixture_context(monkeypatch, artifacts)
    calls = []

    async def execute(task, request, validate):
        value = json.loads(request['messages'][1]['content'][0]['text'])
        calls.append(value)
        if value.get('operation') == 'assemble':
            candidate = next(t for c in value['chapters'] for t in c['targets'])
            assert candidate['template_part_ids'] == ['p1']
            assert next(c for c in value['chapters'] if c['targets'])['title_path'] == [
                '局部附件格式分组', '投标函候选']
            return validate({'chapters': [{'title': '商务文件', 'is_volume': True, 'children': [],
                'targets': [{'kind': 'form', 'purpose': '投标函', 'target_ids': [candidate['target_id']],
                             'template_part_ids': ['p1']}]}]})
        return validate(response(value['sources'][0]['range_id'], value['template_parts'][0]['part_id']))

    result = await pipeline._organize(artifacts, context,
        RequestBudget(model='fixture', context_tokens=131072, output_reserve=16384), execute, [])
    assert len(calls) == 2
    assert result['chapters'][0]['title'] == '商务文件'
    assert result['chapters'][0]['targets'][0]['template_refs'] == [{'source_ref': source, 'selectors': []}]


async def test_failed_assembly_does_not_publish_source_labels_as_formal_volumes(monkeypatch, tmp_path):
    artifacts = ArtifactStore(tmp_path)
    context, _ = fixture_context(monkeypatch, artifacts)

    async def execute(task, request, validate):
        value = json.loads(request['messages'][1]['content'][0]['text'])
        if value.get('operation') == 'assemble':
            raise ModelWorkUnitExhausted(task)
        return validate(response(value['sources'][0]['range_id'], value['template_parts'][0]['part_id']))

    gaps = []
    result = await pipeline._organize(artifacts, context,
        RequestBudget(model='fixture', context_tokens=131072, output_reserve=16384), execute, gaps)
    assert [c['title'] for c in result['chapters']] == ['待整理内容']
    assert all(not c['is_volume'] for c in result['chapters'])
    assert len(result['chapters'][0]['targets']) == 1
    assert any(g['code'] == 'outline_assembly_unavailable' for g in gaps)


def test_context_table_cannot_receive_a_form_fill(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    source = {'source_index_ref': artifacts.put_json({'fixture': True}), 'unit_id': 'review-table'}
    task = {'task_id': 'w1', 'chapter_id': 'chapter-1', 'targets': [
        {'target_id': 'form', 'kind': 'form', 'purpose': '投标函', 'rule_refs': [],
         'source_refs': [source], 'template_refs': [], 'support_origins': ['tender']}]}
    raw = {'blocks': [], 'cell_fills': [{'source_id': 's1', 'row': 0, 'column': 1, 'text': '不应填入评审表',
                          'target_ids': ['t1'], 'support_ids': ['s1']}]}
    value = pipeline.bind_writing_response(task, raw, {'s1': source}, [], support_views=[
        {'support_id': 's1', 'origin': 'tender', 'target_ids': ['t1'], 'text': '评审表', 'grid': {'cells': []}}])
    assert value['cell_fills'] == []


def test_form_blocks_cannot_rewrite_the_selected_non_grid_declaration(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    source = {'source_index_ref': artifacts.put_json({'fixture': True}), 'unit_id': 'declaration'}
    task = {'task_id': 'w1', 'chapter_id': 'chapter-1', 'targets': [
        {'target_id': 'form', 'kind': 'form', 'purpose': '原声明格式', 'rule_refs': [],
         'source_refs': [], 'template_refs': [{'source_ref': source, 'selectors': []}],
         'support_origins': ['tender']}]}
    value = pipeline.bind_writing_response(task, {'blocks': [{'kind': 'paragraph', 'text': '另一份完整声明',
        'target_ids': ['t1'], 'support_ids': ['s1']}]}, {'s1': source}, [], support_views=[
            {'support_id': 's1', 'origin': 'tender', 'target_ids': ['t1'], 'text': '原声明格式', 'grid': None}])
    assert value['blocks'] == []


def test_wire_template_parts_are_request_local():
    from docreader.llm.wire import WirePayload
    wire = WirePayload('outline', {'schema_version': 2, 'template_parts': [
        {'part_id': 'private-part-identity', 'range_ids': ['private-range-identity']}], 'sources': []})
    assert wire.payload['template_parts'] == [{'part_id': 'p1', 'range_ids': ['r1']}]
    assert wire.decode({'template_part_ids': ['p1']}) == {'template_part_ids': ['private-part-identity']}
    assert wire.decode({'template_part_ids': ['private-part-identity']})['template_part_ids'][0].startswith('!unbound:')


def test_related_fact_source_can_be_selected_as_final_block_support(monkeypatch, tmp_path):
    artifacts = ArtifactStore(tmp_path)
    first = {'source_index_ref': artifacts.put_json({'first': True}), 'unit_id': 'general'}
    second = {'source_index_ref': artifacts.put_json({'second': True}), 'unit_id': 'specific'}
    refs = {key: {'kind': 'item', 'item_id': key} for key in ('general-rule', 'specific-rule')}
    rules = [{'rule_id': key, 'item': {'item_id': key, 'text': key,
              'evidence': [{'range_id': range_id, 'quote': text}]}}
             for key, range_id, text in [('general-rule', 'r-general', '通常有效期'),
                                         ('specific-rule', 'r-specific', '本次有效期为180日')]]
    context = SimpleNamespace(payload={'rules': rules, 'issues': [], 'relations': [
        {'relation_id': 'relation', 'from_item_id': 'general-rule', 'to_item_id': 'specific-rule',
         'evidence': []}]}, input={'issues': []}, rules=refs,
        sources={'r-general': [first], 'r-specific': [second]})
    monkeypatch.setattr(pipeline, 'load_outline_source', lambda _artifacts, ref: {
        'document': {'role': 'primary'}, 'unit': {'text': '本次有效期为180日' if ref == second else '通常有效期'}})
    task = {'task_id': 'w1', 'chapter_id': 'chapter-1', 'title_path': ['有效期说明'], 'targets': [
        {'target_id': 'actual', 'kind': 'narrative', 'purpose': '有效期说明', 'rule_refs': [refs['general-rule']],
         'source_refs': [first], 'template_refs': [], 'support_origins': ['tender']}]}
    payload, sources, issues = pipeline._load_task(artifacts, context, task,
        {'response': {'shared_arrangements': []}}, [])
    actual = next(s['support_id'] for s in payload['supports'] if s['text'] == '本次有效期为180日')
    bound = pipeline.bind_writing_response(task, {'blocks': [{'kind': 'paragraph', 'text': '有效期为180日',
        'target_ids': ['t1'], 'support_ids': [actual]}]}, sources, [], support_views=payload['supports'])
    assert not issues
    assert bound['blocks'][0]['source_refs'] == [second]
