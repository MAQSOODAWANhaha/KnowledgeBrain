"""Scoped revisions and unassigned rule evidence cannot widen silently."""

import pytest

from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.authoring import _OutlineContext
import test_assembly_candidate_scope as scope_fixture
from test_assembly_candidate_scope import organize, split, text_of
from test_assembly_source_handoff import BODY, CONDITION, UNRELATED
from test_authoring_templates import create_source


def complete_scope(value):
    response = split(value, positions=1)
    candidate = value['chapters'][0]['targets'][0]
    response['chapters'][0]['targets'][0]['candidate_scope'] = {
        'range_ids': candidate['range_ids'], 'rule_ids': candidate['rule_ids']}
    return response


@pytest.mark.parametrize('kind', ['form', 'attachment'])
async def test_inherited_scope_cannot_change_to_a_non_narrative_carrier(tmp_path, kind):
    def revise(value):
        response = complete_scope(value)
        target = response['chapters'][0]['targets'][0]
        target.pop('candidate_scope')
        target['kind'] = kind
        return response

    artifacts, _, result, _, _ = await organize(
        tmp_path, complete_scope, review=True, revision=revise)
    target = result['chapters'][0]['targets'][0]
    assert target['kind'] == 'narrative'
    assert text_of(artifacts, target) == BODY


def same_native_unit_context(path):
    grid = {'row_count': 2, 'column_count': 1, 'cells': [
        {'row': number, 'column': 0, 'row_span': 1, 'col_span': 1, 'text': text}
        for number, text in enumerate(BODY)]}
    artifacts, index_ref, native = create_source(path, [
        {'grid': grid}, {'text': CONDITION}, {'text': UNRELATED}])
    items = [{'item_id': f'item-{number}', 'text': text, 'evidence': [
        {'range_id': native.ranges[number]['range_id'], 'quote': text}]}
        for number, text in enumerate(BODY)]
    items.append({'item_id': 'condition', 'text': CONDITION, 'evidence': [
        {'range_id': native.ranges[2]['range_id'], 'quote': CONDITION}]})
    frozen = artifacts.put_json({'documents': [
        {**native.document, 'role': 'main', 'source_index_ref': index_ref}]})
    context = _OutlineContext(artifacts, {
        'schema_version': 2, 'run_id': 'scope-same-native-unit', 'source_version': 1, 'domain_revision': 1,
        'frozen_ref': frozen, 'extract_refs': [artifacts.put_json({'items': items})]})
    return artifacts, context


async def test_unassigned_rule_retains_exact_evidence_without_loading_the_whole_native_table(tmp_path, monkeypatch):
    monkeypatch.setattr(scope_fixture, 'context_fixture', same_native_unit_context)

    def choose(value):
        response = split(value, positions=1)
        candidate = value['chapters'][0]['targets'][0]
        # Both exact excerpts are already placed, but the second rule still needs its own position.
        response['chapters'][0]['targets'][0]['candidate_scope']['range_ids'] = candidate['range_ids']
        return response

    artifacts, context, result, _, gaps = await organize(tmp_path, choose)
    target = result['chapters'][-1]['targets'][0]
    assert [ref['item_ref']['item_id'] for ref in target['rule_refs']] == ['item-1']
    assert text_of(artifacts, target) == [BODY[1]]
    task = pipeline.plan_writing_tasks(result)[-1]
    loaded, _, issues = pipeline._load_task(
        artifacts, context, task, {'response': {'shared_arrangements': []}}, [])
    assert not issues
    assert [support['text'] for support in loaded['supports']] == [BODY[1]]
    assert not any(support.get('grid') for support in loaded['supports'])
    assert any(gap['code'] == 'outline_assembly_incomplete' for gap in gaps)


async def test_changed_subject_cannot_bypass_a_previously_split_candidates_explicit_scope(tmp_path):
    def revise(value):
        response = split(value, positions=1)
        target = response['chapters'][0]['targets'][0]
        target.pop('candidate_scope')
        target['subject'] = '原文明确的新主体'
        return response

    artifacts, _, result, _, _ = await organize(tmp_path, split, review=True, revision=revise)
    assert [text_of(artifacts, chapter['targets'][0]) for chapter in result['chapters']] == [
        [BODY[0]], [BODY[1]]]
    assert all('subject' not in chapter['targets'][0] for chapter in result['chapters'])
