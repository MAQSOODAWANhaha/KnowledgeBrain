from copy import deepcopy
import hashlib
import json

import pytest

from docreader.integrations.artifacts import ArtifactIntegrityError
from docreader.tender.authoring_excerpts import source_excerpts_text
from docreader.tender.evidence.references import fragment, selector
from docreader.tender.native_body_scope import NativeBodyScope
from docreader.tender.source_body import bind_source_body
from test_authoring_templates import create_source


def digest(value):
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def case(tmp_path):
    grid = {'row_count': 1, 'column_count': 3, 'cells': [
        {'row': 0, 'column': c, 'row_span': 1, 'col_span': 1, 'text': text}
        for c, text in enumerate(('评分名称', '8', '第一职责。第二职责尾句。'))]}
    artifacts, ref, index = create_source(tmp_path, [{'grid': grid}])
    frozen = artifacts.put_json({'documents': [{**index.document, 'source_index_ref': ref, 'role': 'main'}]})
    snapshot = {'schema_version': 2, 'run_id': 'scope', 'source_version': 1, 'domain_revision': 1, 'frozen_ref': frozen}
    rows = [{'document_id': 'document', 'page': 0, 'line': 0, 'text': '评分名称 8 第一职责。', 'unit_id': 'unit:0'}]
    target = {'target_id': 'chapter-1:target-1', 'kind': 'narrative', 'purpose': rows[0]['text'],
              'rule_refs': [], 'source_refs': [], 'manual_fill': True, 'support_origins': ['tender']}
    owner = {'id': 'source-owner', 'title': '技术响应', 'parent_id': None, 'text': target['purpose'],
             'obligations': '', 'heading': {'document_id': 'document', 'page': 0, 'line': 0},
             'entry': None, 'presentation': 'body', 'subjects': [{'role': '投标人'}]}
    compiled = {'chapters': [owner], 'issues': [], '_source_renderings': [{
        'target_id': target['target_id'], 'chapter_id': 'chapter-1', 'rows': rows,
        'rows_sha256': digest(rows), 'purpose_sha256': hashlib.sha256(target['purpose'].encode()).hexdigest(), 'generated': False}]}
    outline = {'schema_version': 2, 'authoring_input': snapshot, 'chapters': [
        {'chapter_id': 'chapter-1', 'title': '技术响应', 'children': [], 'targets': [target]}], 'issues': []}
    documents = [{'document_id': 'document', 'role': 'main',
                  'pages': [rows[0]['text'] + '\n第二职责尾句。'], 'line_units': [['unit:0', 'unit:0']]}]
    return artifacts, snapshot, documents, compiled, outline, ref, index


def scope(values, **kwargs):
    return NativeBodyScope(*values[:5], **kwargs)


def test_native_domain_keeps_full_cell_tail_and_owner_without_guessing_body_column(tmp_path):
    values = case(tmp_path)
    result = scope(values)
    assert [r['classification'] for r in result.records] == ['native_resolution']
    view = result.selection_view()
    assert len(view.cells) == 3
    assert view.cells[2]['text'] == '第一职责。第二职责尾句。'
    assert view.units[0]['islands'][0]['cell_ids'] == [1, 2, 3]
    binding = result.bindings[view.units[0]['unit_id']]
    assert binding['source_owner_id'] == 'source-owner' and binding['target_id'] == 'chapter-1:target-1'
    payload = result.request_payload(view)
    assert len(payload['source_rows']) == 1 and len(payload['owners']) == 1
    assert payload['owners'][0]['title'] == '技术响应'
    assert not result.pending


def test_cell_relative_selection_becomes_exact_native_excerpt_not_whole_row(tmp_path):
    result = scope(case(tmp_path))
    excerpt = result.cell_excerpt(3, 5, 12)
    assert source_excerpts_text(result.artifacts, [excerpt]) == '第二职责尾句。'
    with pytest.raises(ArtifactIntegrityError):
        result.cell_excerpt(3, 0, 1000)


def test_unmapped_line_on_native_page_is_pending_and_fully_retained(tmp_path):
    values = case(tmp_path)
    values[2][0]['line_units'][0][0] = None
    result = scope(values)
    assert result.records[0]['classification'] == 'layout_pending'
    assert result.pending[0]['reason'] == 'layout_native_ownership_unverified'
    assert result.request_payload(result.selection_view())['source_rows'] == []
    assert result.records[0]['source_islands'][0]['start_line'] == 0
    assert result.records[0]['rendered_rows']


@pytest.mark.parametrize('bad', ['row_bytes', 'row_hash', 'purpose', 'owner', 'origin_missing'])
def test_unmapped_sources_have_named_manual_exit(tmp_path, bad):
    values = case(tmp_path)
    if bad == 'row_bytes':
        values[3]['_source_renderings'][0]['rows'][0]['text'] += '伪造'
        values[3]['_source_renderings'][0]['rows_sha256'] = digest(values[3]['_source_renderings'][0]['rows'])
    if bad == 'row_hash':
        values[3]['_source_renderings'][0]['rows_sha256'] = '0' * 64
    if bad == 'purpose':
        values[4]['chapters'][0]['targets'][0]['purpose'] += '未知来源'
    if bad == 'owner':
        values[4]['chapters'][0]['chapter_id'] = 'chapter-99'
    if bad == 'origin_missing':
        values[3]['_source_renderings'] = []
    result = scope(values)
    assert result.records[0]['classification'].endswith('pending')
    assert result.pending and all(p['outlet'] == 'manual_source_review' and not p['enterprise_confirmed'] for p in result.pending)
    assert not result.selection_view().units


def test_old_source_islands_keep_hidden_rows_without_exposing_them_as_body(tmp_path):
    values = case(tmp_path)
    rows = values[3]['_source_renderings'][0]['rows']
    rows.append({'document_id': 'document', 'page': 0, 'line': 1, 'text': '第二职责尾句。',
                 'render_start': 0, 'render_end': 7, 'rendered': False})
    values[3]['_source_renderings'][0]['rows_sha256'] = digest(rows)
    result = scope(values)
    assert result.records[0]['source_islands'] == [{'document_id': 'document', 'page': 0, 'start_line': 0, 'end_line': 1}]
    assert len(result.request_payload(result.selection_view())['source_rows']) == 2
    assert result.records[0]['rendered_rows'][1][-1] is False


def test_only_explicit_exact_decoration_application_explains_projection_change(tmp_path):
    values = case(tmp_path)
    target = values[4]['chapters'][0]['targets'][0]
    text = values[3]['_source_renderings'][0]['rows'][0]['text']
    target['purpose'] = text[5:]
    app = {'kind': 'source_decoration', 'state': 'presentation_excluded', 'fact_id': 'f',
           'affected_target_ids': [target['target_id']], 'original_ref': values[6].document['original_ref'],
           'original_fragments': [{'page': 0, 'line': 0, 'start': 0, 'end': 5, 'line_text': text,
                                  'text_sha256': hashlib.sha256(text[:5].encode()).hexdigest()}]}
    result = scope(values, decoration_applications=[app])
    assert result.records[0]['classification'] == 'native_resolution'
    assert result.records[0]['presentation_exclusions'][0]['fact_id'] == 'f'
    app['state'] = 'unresolved'
    assert scope(values, decoration_applications=[app]).records[0]['classification'] == 'unmapped_pending'


def test_exact_excerpts_and_body_are_validated_against_purpose(tmp_path):
    values = case(tmp_path)
    target = values[4]['chapters'][0]['targets'][0]
    base = values[6].ranges[0]
    native = base['cell_intervals'][2]
    target['source_excerpts'] = [{'source_ref': {'source_index_ref': values[5], 'unit_id': 'unit:0'},
                                  'selectors': [selector(fragment(base, native['start'], native['end']))]}]
    target['purpose'] = source_excerpts_text(values[0], target['source_excerpts'])
    assert scope(values).records[0]['classification'] == 'source_excerpts_bound'
    target.update(bind_source_body(values[0], values[1], target))
    assert scope(values).records[0]['classification'] == 'source_body_bound'
    target['purpose'] += '错误'
    assert scope(values).records[0]['classification'] == 'unmapped_pending'


def test_form_kind_alone_does_not_create_original_template_obligation(tmp_path):
    values = case(tmp_path)
    target = values[4]['chapters'][0]['targets'][0]
    target['kind'] = 'form'
    assert scope(values).records[0]['classification'] == 'native_resolution'
    assert not target.get('template_refs')
    target['template_refs'] = [{'source_ref': {'source_index_ref': values[5], 'unit_id': 'unit:0'}, 'selectors': []}]
    assert scope(values).records[0]['classification'] == 'template_carried'


def test_generated_and_orphan_targets_are_not_silently_in_native_scope(tmp_path):
    values = case(tmp_path)
    values[3]['_source_renderings'][0]['generated'] = True
    assert scope(values).records[0]['classification'] == 'generated'
    values[4]['unplaced_targets'] = [{**deepcopy(values[4]['chapters'][0]['targets'][0]), 'target_id': 'unplaced'}]
    result = scope(values)
    assert result.records[-1]['classification'] == 'owner_pending'
    assert any(p['target_id'] == 'unplaced' for p in result.pending)


def test_shared_source_rows_and_cells_appear_once_for_distinct_targets(tmp_path):
    values = case(tmp_path)
    target = deepcopy(values[4]['chapters'][0]['targets'][0])
    target['target_id'] = 'chapter-1:target-2'
    target['subject'] = '制造商'
    values[4]['chapters'][0]['targets'].append(target)
    record = deepcopy(values[3]['_source_renderings'][0])
    record['target_id'] = target['target_id']
    values[3]['_source_renderings'].append(record)
    result = scope(values)
    payload = result.request_payload(result.selection_view())
    assert len(payload['units']) == 2 and len(payload['source_rows']) == 1 and len(payload['cells']) == 3
    assert len(result.bindings) == 2


def test_template_carrier_does_not_hide_additional_unbound_purpose(tmp_path):
    values = case(tmp_path)
    target = values[4]['chapters'][0]['targets'][0]
    target['template_refs'] = [{'source_ref': {'source_index_ref': values[5], 'unit_id': 'unit:0'}, 'selectors': []}]
    values[2][0]['pages'][0] += '\n额外独立正文。'
    values[2][0]['line_units'][0].append(None)
    origin = values[3]['_source_renderings'][0]
    origin['rows'].append({'document_id': 'document', 'page': 0, 'line': 2, 'text': '额外独立正文。'})
    target['purpose'] += '\n额外独立正文。'
    origin['rows_sha256'] = digest(origin['rows'])
    origin['purpose_sha256'] = hashlib.sha256(target['purpose'].encode()).hexdigest()
    result = scope(values)
    assert result.records[0]['carrier_valid'] is True
    assert result.records[0]['classification'] == 'unmapped_pending'
    assert result.pending and not result.selection_view().units


def test_provided_owner_mapping_is_bound_to_current_chapter_title(tmp_path):
    values = case(tmp_path)
    values[4]['chapters'][0]['title'] = '另一职责章节'
    result = scope(values, owner_for_chapter={'chapter-1': 'source-owner'})
    assert result.records[0]['classification'] == 'owner_pending'


def test_approved_footer_only_island_does_not_become_unresolved_body(tmp_path):
    values = case(tmp_path)
    values[2][0]['pages'].append('71')
    values[2][0]['line_units'].append([None])
    origin = values[3]['_source_renderings'][0]
    origin['rows'].append({'document_id': 'document', 'page': 1, 'line': 0, 'text': '71'})
    origin['rows_sha256'] = digest(origin['rows'])
    origin['purpose_sha256'] = hashlib.sha256((values[4]['chapters'][0]['targets'][0]['purpose'] + '\n71').encode()).hexdigest()
    app = {'kind': 'source_decoration', 'state': 'presentation_excluded', 'fact_id': 'footer',
           'affected_target_ids': ['chapter-1:target-1'], 'original_ref': values[6].document['original_ref'],
           'original_fragments': [{'page': 1, 'line': 0, 'start': 0, 'end': 2, 'line_text': '71',
                                  'text_sha256': hashlib.sha256(b'71').hexdigest()}]}
    result = scope(values, decoration_applications=[app])
    assert not result.pending
    assert len(result.records[0]['source_islands']) == 2
    assert len(result.selection_view().units[0]['islands']) == 1


def test_exact_excerpt_can_be_upgraded_without_model_call(tmp_path):
    values = case(tmp_path)
    target = values[4]['chapters'][0]['targets'][0]
    target['source_excerpts'] = [scope(values).cell_excerpt(3, 0, 12)]
    target['purpose'] = '第一职责。第二职责尾句。'
    result = scope(values)
    assert result.exact_bindings[target['target_id']] == {
        'parts': [{'source_excerpt': target['source_excerpts'][0], 'join': 'paragraphs'}],
        'context_source_excerpts': []}


def test_cell_offsets_bind_second_native_row_and_multiline_nonfirst_column(tmp_path):
    values = list(case(tmp_path / 'old'))
    grid = {'row_count': 2, 'column_count': 2, 'cells': [
        {'row': r, 'column': c, 'row_span': 1, 'col_span': 1, 'text': text}
        for r, c, text in [(0, 0, 'A'), (0, 1, 'first'), (1, 0, 'prefix'), (1, 1, '尾句。\n续行。')]]}
    artifacts, ref, index = create_source(tmp_path / 'multiline', [{'grid': grid}])
    frozen = artifacts.put_json({'documents': [{**index.document, 'source_index_ref': ref, 'role': 'main'}]})
    values[0] = artifacts
    values[1] = {**values[1], 'frozen_ref': frozen}
    values[4]['authoring_input'] = values[1]
    result = scope(values)
    excerpt = result.cell_excerpt(4, 4, 7)
    assert source_excerpts_text(artifacts, [excerpt]) == '续行。'
    assert excerpt['selectors'][0]['start'] == 13  # native row prefix + separator + cell-relative offset


def test_response_table_carrier_does_not_hide_additional_purpose(tmp_path):
    values = case(tmp_path)
    target = values[4]['chapters'][0]['targets'][0]
    excerpt = scope(values).cell_excerpt(3, 0, 12)
    target['response_table'] = {'rows': [{'source_excerpts': [excerpt]}]}
    result = scope(values)
    assert result.records[0]['carrier_valid'] is True
    assert result.records[0]['classification'] == 'unmapped_pending'


def test_request_cannot_substitute_cell_text_or_change_unit_scope(tmp_path):
    result = scope(case(tmp_path))
    view = result.selection_view()
    view.cells[2]['text'] = '另一段未批准正文。'
    with pytest.raises(ArtifactIntegrityError):
        result.request_payload(view)


def test_negative_source_coordinates_do_not_alias_the_last_page(tmp_path):
    values = case(tmp_path)
    row = values[3]['_source_renderings'][0]['rows'][0]
    row['page'] = -1
    values[3]['_source_renderings'][0]['rows_sha256'] = digest(values[3]['_source_renderings'][0]['rows'])
    result = scope(values)
    assert result.records[0]['classification'] == 'unmapped_pending'
