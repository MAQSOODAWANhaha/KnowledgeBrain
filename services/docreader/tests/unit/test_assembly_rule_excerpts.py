"""Assembly can select complete frozen evidence even when a source pass omitted it."""

from copy import deepcopy
from io import BytesIO
import json

from docx import Document
import pytest

from docreader.llm.budget import RequestBudget
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.authoring import _OutlineContext
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.authoring_excerpts import load_source_excerpt
from docreader.tests.submission_responses import assemble_independent_candidates
from test_assembly_source_handoff import CONDITION, QUOTES, UNBOUND, UNRELATED
from test_authoring_templates import create_context, create_source


BODY = ['第一项要求。', '第二项要求：仅限已声明场景，原厂应在五日内提供盖章证明。']


def context_fixture(tmp_path):
    artifacts, index_ref, native = create_source(tmp_path, [
        {'text': text, 'heading': f'来源{number}'} for number, text in enumerate([*BODY, CONDITION, UNRELATED])])
    base = create_context(artifacts, index_ref, native)
    items = [{'item_id': f'item-{number}', 'text': text, 'evidence': [{
        'range_id': native.ranges[number]['range_id'], 'quote': '要求'}]} for number, text in enumerate(BODY)]
    items.append({'item_id': 'condition', 'text': CONDITION, 'evidence': [
        *({'range_id': native.ranges[2]['range_id'], 'quote': quote} for quote in QUOTES),
        {'range_id': 'unavailable-range', 'quote': UNBOUND}]})
    return artifacts, _OutlineContext(artifacts, {
        **base.input, 'extract_refs': [artifacts.put_json({'items': items})]})


def records(value, field, identity):
    entries = value[field]
    return [{identity: key, **entry} for key, entry in entries.items()] if isinstance(entries, dict) else entries


def source_text(value, entry):
    return entry.get('text', '\n'.join(value.get('text_pool', [])[line] if type(line) is int else line
                                     for line in entry.get('text_lines', [])))


def rule(value, text):
    return next(entry for field in ('candidate_rules', 'unassigned_rules')
                for entry in records(value, field, 'rule_id') if entry['rule']['text'] == text)


async def organize(tmp_path, monkeypatch, choose, *, compact=False):
    artifacts, context = context_fixture(tmp_path)
    before, original_sources = deepcopy(context.payload), deepcopy(context.sources)
    calls = []
    original_fits = pipeline._fits
    if compact:
        monkeypatch.setattr(pipeline, '_fits', lambda budget, task, value, **options:
            False if value.get('operation') == 'assemble' else original_fits(budget, task, value, **options))

    async def execute(task, request, validate):
        value = json.loads(request['messages'][1]['content'][0]['text'])
        calls.append(deepcopy(value))
        if value.get('operation') == 'assemble':
            assert (value.get('assembly_view') == 'compact_v1') is compact
            return validate(choose(value))
        sources = {entry['text']: entry['range_id'] for entry in value['sources']}
        rules = {entry['item']['text']: entry['rule_id'] for entry in value['rules']}
        return validate({'schema_version': 2, 'chapters': [{'title': '原有响应', 'targets': [{
            'kind': 'narrative', 'purpose': '只安排第一条，第二条及共同条件留给全局组织。',
            'rule_ids': [rules[BODY[0]]], 'range_ids': [sources[BODY[0]]]}]}], 'findings': []})

    result = await pipeline._organize(artifacts, context,
        RequestBudget(model='fixture', context_tokens=131072), execute, [])
    assert context.payload == before
    assert all(context.sources[identity] == refs for identity, refs in original_sources.items())
    assert len(calls) == 2, 'Source evidence registration must not add model calls.'
    return artifacts, result


@pytest.mark.parametrize('compact', [False, True])
async def test_unassigned_rule_complete_sources_can_form_a_response_row_with_common_conditions(
        tmp_path, monkeypatch, compact):
    def choose(value):
        obligation, condition = rule(value, BODY[1]), rule(value, CONDITION)
        evidence = obligation['rule']['evidence'][0]
        assert evidence['quote'] == '要求'
        assert evidence.get('range_ids'), 'Unassigned rule evidence needs selectable exact source references.'
        contexts = condition['rule']['evidence'][0].get('range_ids')
        assert contexts, 'Known common conditions need selectable exact source references.'
        response = assemble_independent_candidates(value)
        response['chapters'].append({'title': '补全技术响应', 'targets': [{
            'kind': 'narrative', 'purpose': '保留完整原要求及共同条件，应答等待真实资料。',
            'rule_ids': [obligation['rule_id'], condition['rule_id']],
            'response_table': {'context_range_ids': contexts, 'rows': [{
                'range_ids': evidence['range_ids'], 'rule_ids': [obligation['rule_id']]}]}}]})
        return response

    artifacts, result = await organize(tmp_path, monkeypatch, choose, compact=compact)
    target, = result['chapters'][-1]['targets']
    table = target['response_table']
    assert [load_source_excerpt(artifacts, ref)['text'] for ref in table['rows'][0]['source_excerpts']] == [BODY[1]]
    assert [load_source_excerpt(artifacts, ref)['text'] for ref in table['context_source_excerpts']] == [CONDITION]
    assert [ref['item_ref']['item_id'] for ref in table['rows'][0]['rule_refs']] == ['item-1']
    before = deepcopy(result)
    exported = export_authoring_document(artifacts, result, [])
    document = Document(BytesIO(artifacts.read_bytes(exported['docx_ref'])))
    assert len(document.tables) == 1
    assert [[cell.text for cell in row.cells] for row in document.tables[0].rows] == [
        ['招标要求', '投标响应'], [BODY[1], '']]
    assert [paragraph.text for paragraph in document.paragraphs].count(CONDITION) == 1
    assert document.element.xml.index(CONDITION) < document.element.xml.index('招标要求')
    assert UNBOUND not in document.element.xml and UNRELATED not in document.element.xml
    assert result == before


@pytest.mark.parametrize('compact', [False, True])
async def test_rule_source_registration_reuses_existing_aliases_and_preserves_distinct_domains(
        tmp_path, monkeypatch, compact):
    def choose(value):
        existing = rule(value, BODY[0])['rule']['evidence'][0]
        omitted = rule(value, BODY[1])['rule']['evidence'][0]
        conditions = rule(value, CONDITION)['rule']['evidence']
        assert existing.get('range_ids') == ['x1'], 'Candidate evidence must reuse its original source alias.'
        assert omitted.get('range_ids') == ['x2']
        assert conditions[0]['range_ids'] == conditions[1]['range_ids'] == ['x3']
        assert conditions[2].get('range_ids', []) == []
        assert conditions[2]['quote'] == UNBOUND
        excerpts = records(value, 'source_excerpts', 'range_id')
        assert [(entry['range_id'], source_text(value, entry)) for entry in excerpts] == [
            ('x1', BODY[0]), ('x2', BODY[1]), ('x3', CONDITION)]
        assert all(UNRELATED not in source_text(value, entry) for entry in excerpts)
        return assemble_independent_candidates(value)

    _, result = await organize(tmp_path, monkeypatch, choose, compact=compact)
    assert len(result['chapters']) == 1 and len(result['chapters'][0]['targets']) == 1
    assert 'response_table' not in result['chapters'][0]['targets'][0]


@pytest.mark.parametrize('compact', [False, True])
async def test_unbindable_evidence_is_not_replaced_with_a_quote_or_an_unrelated_source(
        tmp_path, monkeypatch, compact):
    def choose(value):
        unknown = rule(value, CONDITION)['rule']['evidence'][2]
        assert unknown['quote'] == UNBOUND and not unknown.get('range_ids')
        texts = [source_text(value, entry) for entry in records(value, 'source_excerpts', 'range_id')]
        assert UNBOUND not in texts and UNRELATED not in texts
        return assemble_independent_candidates(value)

    await organize(tmp_path, monkeypatch, choose, compact=compact)
