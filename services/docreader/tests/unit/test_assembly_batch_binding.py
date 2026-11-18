"""Every local assembly response is bound to the actual request's visible references."""

from copy import deepcopy
import json

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.authoring import _OutlineContext
from docreader.tests.submission_responses import assemble_independent_candidates
from test_authoring_templates import create_context, create_source


TEXTS = ['第一包的完整要求。', '第二包的独立声明：签章____。', '共同提交条件。', '尚未安排的独立要求。']


class Inspected(BaseException):
    pass


async def inspect_assembly(tmp_path, monkeypatch, inspect, *, phase='local', hide_shared=False, hide_excerpt=False):
    artifacts, index_ref, native = create_source(tmp_path, [{'text': text} for text in TEXTS])
    base = create_context(artifacts, index_ref, native)
    items = [{'item_id': f'item-{i}', 'text': text,
              'evidence': [{'range_id': native.ranges[i]['range_id'], 'quote': text}]}
             for i, text in enumerate(TEXTS)]
    context = _OutlineContext(artifacts, {**base.input, 'extract_refs': [artifacts.put_json({'items': items})]})
    before = deepcopy(context.payload)
    captured = {}

    def two_groups(payload, budget):
        captured['full'] = deepcopy(payload)
        assert len(payload['chapters']) == 2
        first = pipeline._assembly_subset(payload, payload['chapters'][:1], include_unassigned=False)
        second = pipeline._assembly_subset(payload, payload['chapters'][1:], include_unassigned=True)
        if hide_shared:
            first['submission_rules'] = []
            first['submission_sources'] = []
        if hide_excerpt:
            first['source_excerpts'] = []
        return [first, second]

    monkeypatch.setattr(pipeline, '_assembly_batches', two_groups)

    async def execute(task, request, validate):
        value = json.loads(request['messages'][1]['content'][0]['text'])
        if value.get('operation') == 'assemble':
            current_phase = 'global' if value.get('assembly_bodies') == 'retained_on_host' else 'local'
            if current_phase == phase:
                inspect(value, captured['full'], validate, captured['source'])
                raise Inspected
            return validate(assemble_independent_candidates(value))
        ranges = {entry['text']: entry['range_id'] for entry in value['sources']}
        rules = {entry['item']['text']: entry['rule_id'] for entry in value['rules']}
        part = next(p['part_id'] for p in value['template_parts'] if ranges[TEXTS[1]] in p['range_ids'])
        raw = {'schema_version': 2, 'chapters': [
            {'title': '第一包', 'targets': [{'kind': 'narrative', 'purpose': TEXTS[0],
                'rule_ids': [rules[TEXTS[0]]], 'range_ids': [ranges[TEXTS[0]]]}]},
            {'title': '第二包', 'targets': [{'kind': 'form', 'purpose': TEXTS[1],
                'rule_ids': [rules[TEXTS[1]]], 'range_ids': [ranges[TEXTS[1]]], 'template_part_ids': [part]}]},
        ], 'constraint_rule_ids': [rules[TEXTS[2]]], 'constraint_range_ids': [ranges[TEXTS[2]]], 'findings': []}
        bound = validate(raw)
        captured['source'] = deepcopy(bound)
        return bound

    with pytest.raises(Inspected):
        await pipeline._organize(artifacts, context, RequestBudget(model='fixture', context_tokens=131072), execute, [])
    assert context.payload == before


async def test_local_assembly_retains_only_its_own_omitted_candidates(tmp_path, monkeypatch):
    def inspect(value, full, validate, source):
        assert len(value['chapters']) == 1 and len(full['chapters']) == 2
        assembled = validate({'chapters': []})
        visible = {t['target_id'] for c in value['chapters'] for t in c['targets']}
        assert {alias for aliases in assembled[6].values() for alias in aliases} == visible
        retained = [t for c in pipeline._flatten(assembled[0]) for t in c['targets']]
        assert len(retained) == 1
        assert retained[0]['source_refs'] == source['chapters'][0]['targets'][0]['source_refs']
        assert retained[0]['rule_refs'] == source['chapters'][0]['targets'][0]['rule_refs']

    await inspect_assembly(tmp_path, monkeypatch, inspect)


@pytest.mark.parametrize('reference', ['target', 'related_target', 'candidate_rule', 'unassigned_rule',
                                      'excerpt', 'context_excerpt', 'scope_excerpt', 'template',
                                      'template_span', 'template_header', 'submission_rule', 'submission_source'])
async def test_local_assembly_rejects_references_visible_only_in_other_requests(tmp_path, monkeypatch, reference):
    def inspect(value, full, validate, source):
        local = value['chapters'][0]['targets'][0]
        other = full['chapters'][1]['targets'][0]
        target = {'kind': 'narrative', 'purpose': '本包响应', 'target_ids': [local['target_id']]}
        response = {'chapters': [{'title': '本包', 'targets': [target]}]}
        if reference == 'target':
            target['target_ids'] = [other['target_id']]
        elif reference == 'related_target':
            target['related_target_ids'] = [other['target_id']]
        elif reference == 'candidate_rule':
            target['rule_ids'] = [other['rule_ids'][0]]
        elif reference == 'unassigned_rule':
            assert not value['unassigned_rules']
            target['rule_ids'] = [full['unassigned_rules'][0]['rule_id']]
        elif reference in {'excerpt', 'context_excerpt'}:
            identity = other['range_ids'][0]
            assert identity not in {entry['range_id'] for entry in value['source_excerpts']}
            target['response_table'] = {'rows': [{'range_ids': [
                identity if reference == 'excerpt' else local['range_ids'][0]], 'rule_ids': []}]}
            if reference == 'context_excerpt':
                target['response_table']['context_range_ids'] = [identity]
        elif reference == 'scope_excerpt':
            assert not value['source_excerpts']
            target['candidate_scope'] = {'range_ids': [local['range_ids'][0]], 'rule_ids': []}
        elif reference == 'template':
            assert not value['template_parts']
            target.update(kind='form', template_part_ids=[other['template_part_ids'][0]])
        elif reference == 'template_span':
            part = other['template_part_ids'][0]
            target.update(kind='form', template_spans=[{'start_part_id': part, 'end_part_id': part}])
        elif reference == 'template_header':
            response['table_headers'] = [{'part_id': other['template_part_ids'][0], 'header_row_count': 0}]
        elif reference == 'submission_rule':
            assert not value['submission_rules']
            response['constraint_rule_ids'] = [full['submission_rules'][0]['rule_id']]
        else:
            assert not value['submission_sources']
            target['source_ids'] = [full['submission_sources'][0]['source_id']]
        with pytest.raises(ModelContractError) as error:
            validate(response)
        fields = {'target': ['target_ids', 0], 'related_target': ['related_target_ids', 0],
                  'candidate_rule': ['rule_ids', 0], 'unassigned_rule': ['rule_ids', 0],
                  'excerpt': ['response_table', 'rows', 0, 'range_ids', 0],
                  'context_excerpt': ['response_table', 'context_range_ids', 0],
                  'scope_excerpt': ['candidate_scope', 'range_ids', 0],
                  'template': ['template_part_ids', 0], 'template_span': ['template_spans', 0, 'start_part_id'],
                  'submission_source': ['source_ids', 0]}
        path = (['constraint_rule_ids', 0] if reference == 'submission_rule' else
                ['table_headers', 0, 'part_id'] if reference == 'template_header' else
                ['chapters', 0, 'targets', 0, *fields[reference]])
        assert any(item['path'] == path for item in error.value.errors)

    await inspect_assembly(tmp_path, monkeypatch, inspect,
                           hide_shared=reference in {'submission_rule', 'submission_source'},
                           hide_excerpt=reference == 'scope_excerpt')


async def test_global_assembly_still_accepts_the_complete_visible_candidate_set(tmp_path, monkeypatch):
    def inspect(value, full, validate, source):
        assert len(value['chapters']) == len(full['chapters']) == 2
        assembled = validate(assemble_independent_candidates(value))
        provided = {t['target_id'] for c in value['chapters'] for t in c['targets']}
        assert {alias for aliases in assembled[6].values() for alias in aliases} == provided
        assert not assembled[1]

    await inspect_assembly(tmp_path, monkeypatch, inspect, phase='global')
