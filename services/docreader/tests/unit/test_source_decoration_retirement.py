"""Only source-only decoration targets may leave the response set, with full audit."""

from copy import deepcopy
from hashlib import sha256

import pytest

from docreader.tender import reviewed_source_facts as reviewed
from docreader.tender.authoring_excerpts import whole_source_excerpts
from docreader.tender.directory_compile import outline_from_compiled
from docreader.tender.schema import AuthoringOutline
from docreader.tender.source_presentation import _rows_sha
from test_source_presentation import fixture, load


def pure_decoration(tmp_path, *, split_facts=False):
    artifacts, snapshot, documents, manifest, compiled, _ = data = fixture(tmp_path)
    if split_facts:
        original = manifest['facts'][0]
        manifest['facts'] = []
        for start, end in [(0, 7), (7, len('TENDER HEADER'))]:
            fact = deepcopy(original)
            fragment = fact['layout_fragments'][0]
            fragment.update(start=start, end=end,
                text_sha256=sha256('TENDER HEADER'[start:end].encode()).hexdigest())
            chars = fragment['original_chars']
            first = chars['start']
            chars.update(start=first + start, end=first + end, text_sha256=fragment['text_sha256'])
            manifest['facts'].append(fact)
    compiled['chapters'][0]['text'] = 'TENDER HEADER'
    compiled['chapters'][0]['_source_body_rows'] = [
        row for row in compiled['chapters'][0]['_source_body_rows'] if row['line'] == 0]
    outline = outline_from_compiled(snapshot, compiled)
    target = outline['chapters'][0]['targets'][0]
    return outline, compiled, target, load(data), artifacts, snapshot


def test_retirement_keeps_complete_original_target_and_position_without_false_response(tmp_path):
    outline, compiled, target, facts, _, _ = pure_decoration(tmp_path)
    before = deepcopy(target)
    origin = deepcopy(compiled['_source_renderings'][0])
    preserved = {**deepcopy(target), 'target_id': 'chapter-1:target-7', 'purpose': 'Capacity 80'}
    outline['chapters'][0]['targets'].insert(0, preserved)
    compiled['_source_renderings'].append({'target_id': preserved['target_id'],
        'chapter_id': 'chapter-1', 'rows': [], 'generated': True})
    reviewed.bind_reviewed_outline_facts(outline, compiled, facts)

    assert outline['chapters'][0]['targets'] == [preserved]
    record, = compiled['_reviewed_source_fact_applications']
    assert record['state'] == 'presentation_excluded'
    assert record['removed_target_ids'] == [target['target_id']]
    assert record['retired_targets'] == [{
        'chapter_id': 'chapter-1', 'target_ordinal': 1, 'target': before,
        'source_rendering': origin, 'decoration_fact_ids': [facts[0]['fact_id']]}]
    assert record['retired_targets'][0]['target'] is not target
    assert before['manual_fill'] is True
    assert not any(issue['code'] == 'outline_directory_reviewed_fact_pending' for issue in outline['issues'])


@pytest.mark.parametrize('duty', [
    'rule_refs', 'source_refs', 'source_excerpts', 'source_body_ref', 'template_refs', 'subject',
    'response_table', 'response_form_pending', 'related_target_ids', 'incoming_related_target_ids',
    'unplaced_incoming_related_target_ids'])
def test_target_with_any_response_duty_is_preserved_and_named_pending(tmp_path, duty):
    outline, compiled, target, facts, artifacts, snapshot = pure_decoration(tmp_path)
    index_ref = artifacts.read_json(snapshot['frozen_ref'])['documents'][0]['source_index_ref']
    source_ref = {'source_index_ref': index_ref, 'unit_id': 'unit:0'}
    excerpts = whole_source_excerpts(artifacts, source_ref)
    window = {'source_index_ref': index_ref, 'document_id': 'document',
        'page': 0, 'start_line': 0, 'end_line': 0}
    if duty in {'incoming_related_target_ids', 'unplaced_incoming_related_target_ids'}:
        entries = (outline.setdefault('unplaced_targets', []) if duty.startswith('unplaced')
                   else outline['chapters'][0]['targets'])
        entries.append({**deepcopy(target),
            'target_id': 'chapter-1:target-7', 'purpose': 'Actual required response',
            'related_target_ids': [target['target_id']]})
        compiled['_source_renderings'].append({'target_id': 'chapter-1:target-7',
            'chapter_id': 'chapter-1', 'rows': [], 'generated': True})
    elif duty == 'subject':
        target[duty] = 'Required bidder'
    elif duty == 'response_table':
        target[duty] = {'rows': [{'row_id': 'required-row', 'source_excerpts': excerpts}]}
    elif duty == 'response_form_pending':
        target[duty] = {'source_binding': '0' * 64, 'table_source_ref': source_ref,
            'mandate_windows': [window], 'source_windows': [window], 'reason': 'selection_incomplete'}
    elif duty == 'rule_refs':
        target[duty] = [{'item_ref': {'artifact_ref': artifacts.put_json({'item_id': 'required-item'}),
            'item_id': 'required-item'}}]
    elif duty == 'source_refs':
        target[duty] = [source_ref]
    elif duty == 'source_excerpts':
        target[duty] = excerpts
    elif duty == 'source_body_ref':
        target['source_body_contract'] = 'source_bound_native_body_v1'
        target[duty] = artifacts.put_json({'immutable_body': 'TENDER HEADER'})
    elif duty == 'template_refs':
        target[duty] = [{'source_ref': source_ref, 'selectors': []}]
    else:
        target[duty] = ['actual-required-reference']
    before = deepcopy(outline['chapters'])
    AuthoringOutline.model_validate(outline)

    reviewed.bind_reviewed_outline_facts(outline, compiled, facts)

    assert outline['chapters'] == before
    record, = compiled['_reviewed_source_fact_applications']
    assert record['state'] == 'unresolved'
    assert record['removed_target_ids'] == []
    assert record.get('retired_targets', []) == []
    assert record['affected_target_ids'] == []
    assert any(issue['code'] == 'outline_directory_reviewed_fact_pending' for issue in outline['issues'])


def test_uncovered_signature_characters_keep_response_target_and_original_sources(tmp_path):
    outline, compiled, target, facts, _, _ = pure_decoration(tmp_path)
    row = {'document_id': 'document', 'page': 0, 'line': 2, 'text': 'Signature: ______'}
    origin = compiled['_source_renderings'][0]
    origin['rows'].append(row)
    origin['rows_sha256'] = _rows_sha(origin['rows'])
    target['purpose'] += '\n' + row['text']
    origin['purpose_sha256'] = sha256(target['purpose'].encode()).hexdigest()
    source_before = deepcopy(origin)

    reviewed.bind_reviewed_outline_facts(outline, compiled, facts)

    assert outline['chapters'][0]['targets'] == [target]
    assert target['purpose'] == 'Signature: ______'
    assert target['manual_fill'] is True
    assert compiled['_source_renderings'][0] == source_before
    record, = compiled['_reviewed_source_fact_applications']
    assert record['state'] == 'presentation_excluded' and record['removed_target_ids'] == []
    assert record.get('retired_targets', []) == []


def test_several_retirements_keep_original_ordinals_and_complete_combined_fact_coverage(tmp_path):
    outline, compiled, target, facts, _, _ = pure_decoration(tmp_path, split_facts=True)
    second = {**deepcopy(target), 'target_id': 'chapter-1:target-9'}
    substantive = {**deepcopy(target), 'target_id': 'chapter-1:target-7', 'purpose': 'Capacity 80'}
    outline['chapters'][0]['targets'].extend([substantive, second])
    second_origin = {**deepcopy(compiled['_source_renderings'][0]), 'target_id': second['target_id']}
    compiled['_source_renderings'].extend([{'target_id': substantive['target_id'],
        'chapter_id': 'chapter-1', 'rows': [], 'generated': True}, second_origin])
    original = deepcopy(outline['chapters'][0]['targets'])

    reviewed.bind_reviewed_outline_facts(outline, compiled, facts)

    assert outline['chapters'][0]['targets'] == [substantive]
    for record in compiled['_reviewed_source_fact_applications']:
        assert record['state'] == 'presentation_excluded'
        assert record['removed_target_ids'] == [target['target_id'], second['target_id']]
        assert [entry['target_ordinal'] for entry in record['retired_targets']] == [0, 2]
        assert [entry['target'] for entry in record['retired_targets']] == [original[0], original[2]]
        assert all(entry['decoration_fact_ids'] == sorted(fact['fact_id'] for fact in facts)
                   for entry in record['retired_targets'])
