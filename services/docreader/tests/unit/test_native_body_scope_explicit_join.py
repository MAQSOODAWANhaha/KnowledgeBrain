from copy import deepcopy
import hashlib
from typing import Any

import pytest

from docreader.tender.evidence.references import selector
from docreader.tender.native_body_scope import NativeBodyScope
from docreader.tender.source_body import bind_source_body, load_source_body
from test_authoring_templates import create_source
from test_native_body_scope import case


def explicit_case(tmp_path):
    values: list[Any] = list(case(tmp_path / 'old'))
    artifacts, ref, index = create_source(tmp_path / 'joined', [
        {'text': '第一段'}, {'text': '第二段'}, {'text': '来源名称'}])
    snapshot = {**values[1], 'frozen_ref': artifacts.put_json({'documents': [
        {**index.document, 'source_index_ref': ref, 'role': 'main'}]})}
    excerpts = [{'source_ref': {'source_index_ref': ref, 'unit_id': base['unit_id']},
                 'selectors': [selector(base)]} for base in index.ranges]
    target = values[4]['chapters'][0]['targets'][0]
    target.update(purpose='第一段第二段', source_excerpts=excerpts[:2])
    values[0], values[1] = artifacts, snapshot
    values[4]['authoring_input'] = snapshot
    app = {'kind': 'independent_body_response', 'state': 'body_bound', 'target_ids': [target['target_id']],
           'source_join': 'continuation', 'source_excerpts': deepcopy(excerpts[:2]),
           'source_text_sha256': hashlib.sha256(target['purpose'].encode()).hexdigest(),
           'context_source_excerpts': deepcopy(excerpts[2:])}
    values[3]['_reviewed_source_fact_applications'] = [app]
    return values, target, app


def test_explicit_current_body_receipt_preserves_joins_and_context(tmp_path):
    values, target, app = explicit_case(tmp_path)
    scope = NativeBodyScope(*values[:5])
    assert scope.records[0]['classification'] == 'source_excerpts_bound'
    binding = scope.exact_bindings[target['target_id']]
    assert [p['join'] for p in binding['parts']] == ['paragraphs', 'continuation']
    assert binding['context_source_excerpts'] == app['context_source_excerpts']
    updated = {**target, 'source_context_excerpts': binding['context_source_excerpts']}
    updated = bind_source_body(values[0], values[1], updated, binding['parts'])
    assert load_source_body(values[0], values[1], updated) == '第一段第二段'


@pytest.mark.parametrize('bad', ['missing_receipt', 'wrong_hash', 'wrong_reference', 'wrong_target', 'unresolved', 'conflict'])
def test_no_join_is_inferred_from_matching_strings_without_a_consistent_receipt(tmp_path, bad):
    values, target, app = explicit_case(tmp_path)
    if bad == 'missing_receipt':
        values[3]['_reviewed_source_fact_applications'] = []
    if bad == 'wrong_hash':
        app['source_text_sha256'] = '0' * 64
    if bad == 'wrong_reference':
        app['source_excerpts'] = app['source_excerpts'][:1]
    if bad == 'wrong_target':
        app['target_ids'] = ['unrelated']
    if bad == 'unresolved':
        app['state'] = 'unresolved'
    if bad == 'conflict':
        other = deepcopy(app)
        other['source_join'] = 'paragraphs'
        values[3]['_reviewed_source_fact_applications'].append(other)
    scope = NativeBodyScope(*values[:5])
    assert scope.records[0]['classification'] == 'unmapped_pending'
    assert target['target_id'] not in scope.exact_bindings and scope.pending


def default_join_case(tmp_path):
    values, target, app = explicit_case(tmp_path)
    target['purpose'] = '第一段\n第二段'
    app['source_text_sha256'] = hashlib.sha256(target['purpose'].encode()).hexdigest()
    app['source_join'] = None
    return values, target, app


def test_exact_default_paragraphs_keeps_explicit_context_without_inventing_continuation(tmp_path):
    values, target, app = default_join_case(tmp_path)
    scope = NativeBodyScope(*values[:5])
    binding = scope.exact_bindings[target['target_id']]
    assert [p['join'] for p in binding['parts']] == ['paragraphs', 'paragraphs']
    assert binding['context_source_excerpts'] == app['context_source_excerpts']


@pytest.mark.parametrize('bad', ['public_conflict', 'receipt_conflict', 'wrong_hash', 'overlap'])
def test_default_body_context_cannot_override_public_context_or_conflicting_receipts(tmp_path, bad):
    from docreader.tender.evidence.references import EvidenceRepository, fragment, selector
    values, target, app = default_join_case(tmp_path)
    if bad == 'public_conflict':
        context = deepcopy(app['context_source_excerpts'][0])
        index = EvidenceRepository(values[0]).source_index(context['source_ref']['source_index_ref'])
        base = index.by_id[context['selectors'][0]['source_range_id']]
        context['selectors'] = [selector(fragment(base, base['start'], base['start'] + 2))]
        target['source_context_excerpts'] = [context]
    if bad == 'receipt_conflict':
        other = deepcopy(app)
        other['context_source_excerpts'] = []
        values[3]['_reviewed_source_fact_applications'].append(other)
    if bad == 'wrong_hash':
        app['source_text_sha256'] = '0' * 64
    if bad == 'overlap':
        app['context_source_excerpts'] = deepcopy(target['source_excerpts'][:1])
    scope = NativeBodyScope(*values[:5])
    assert scope.records[0]['classification'] == 'unmapped_pending'
    assert target['target_id'] not in scope.exact_bindings and scope.pending
