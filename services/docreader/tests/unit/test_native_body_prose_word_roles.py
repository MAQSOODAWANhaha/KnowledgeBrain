from copy import deepcopy
from io import BytesIO

from docx import Document

from docreader.llm.request import ModelContractError
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.native_body_scope import NativeBodyScope
from docreader.tender.source_body import bind_source_body
from test_native_body_prose import prose_case


def word_for(values, scope, body, context):
    view = scope.selection_view()
    island = view.units[0]['islands'][0]['island_id']
    accepted = view.accept({'native_choices': [{'unit_id': view.units[0]['unit_id'], 'islands': [
        {'island_id': island, 'state': 'selected', 'body': body, 'context': context}]}]})
    unit = accepted['units'][0]
    target = deepcopy(values[4]['chapters'][0]['targets'][0])
    target['source_context_excerpts'] = [scope.cell_excerpt(p['cell_id'], p['start'], p['end']) for p in unit['context']]
    parts = [{'source_excerpt': scope.cell_excerpt(p['cell_id'], p['start'], p['end']), 'join': p['source_join']}
             for selected in unit['islands'] for p in selected['body']]
    bound = bind_source_body(values[0], values[1], target, parts)
    outline = deepcopy(values[4])
    outline['chapters'][0]['targets'][0] = bound
    result = export_authoring_document(values[0], outline, [])
    return [p.text for p in Document(BytesIO(values[0].read_bytes(result['docx_ref']))).paragraphs]


def test_next_independent_requirement_is_not_automatically_inserted_into_current_word(tmp_path):
    values, _, _ = prose_case(tmp_path, native='第一职责。另行提交银行保函。')
    scope = NativeBodyScope(*values[:5])
    cid = scope.selection_view().cells[0]['cell_id']
    paragraphs = word_for(values, scope, [[cid, 1, 1, 0]], [])
    assert '第一职责。' in paragraphs
    assert all('另行提交银行保函' not in p for p in paragraphs)


def test_only_explicit_context_is_rendered_before_the_blank_response(tmp_path):
    values, _, _ = prose_case(tmp_path, purpose='来源说明。第一职责。',
                               native='来源说明。第一职责。另行提交银行保函。')
    scope = NativeBodyScope(*values[:5])
    cid = scope.selection_view().cells[0]['cell_id']
    paragraphs = word_for(values, scope, [[cid, 2, 2, 0]], [[cid, 1, 1]])
    assert '第一职责。' in paragraphs and '来源说明。' in paragraphs
    assert '原文上下文（非企业应答）：' in paragraphs
    assert all('银行保函' not in p for p in paragraphs)


def test_missing_prior_requirement_cannot_be_hidden_by_omitting_context(tmp_path):
    values, _, _ = prose_case(tmp_path, purpose='来源说明。第一职责。', native='来源说明。第一职责。邻项。')
    scope = NativeBodyScope(*values[:5])
    view = scope.selection_view()
    cid = view.cells[0]['cell_id']
    accepted = view.accept({'native_choices': [{'unit_id': view.units[0]['unit_id'], 'islands': [
        {'island_id': view.units[0]['islands'][0]['island_id'], 'state': 'selected',
         'body': [[cid, 2, 2, 0]], 'context': []}]}]})
    assert not accepted['complete']
    assert accepted['pending'][0]['reason'] == 'native_body_anchor_uncovered'
    assert accepted['units'][0]['islands'][0]['body'] == []


def test_prose_output_bound_includes_explicit_context_and_coverage_states():
    import itertools
    import json
    from docreader.tender.native_body_selection import NativeBodySelectionView
    view = NativeBodySelectionView([
        {'cell_id': 10000, 'document_id': 'd', 'source_kind': 'native_prose', 'native_unit_id': 'u',
         'native_range_id': 'r', 'page': 0, 'text': '甲。乙。丙。'}
    ], [{'unit_id': 42, 'anchor_spans': [[10000, 0, 6]], 'islands': [{'island_id': 7, 'cell_ids': [10000]}]}])
    intervals = list(itertools.combinations_with_replacement((1, 2, 3), 2))
    contexts = [[], *[[[10000, *i]] for i in intervals],
                *[[[10000, *a], [10000, *b]] for a in intervals for b in intervals if a[1] < b[0]]]
    accepted = 0
    for body in intervals:
        for context in contexts:
            raw = {'native_choices': [{'unit_id': 42, 'islands': [
                {'island_id': 7, 'state': 'selected', 'body': [[10000, *body, 0]], 'context': context}]}]}
            try:
                view.accept(raw)
            except ModelContractError:
                continue
            accepted += 1
            assert len(json.dumps(raw, ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()) <= view.output_bound()['token_upper_bound']
    assert accepted > 0
    schema = view.response_schema()['properties']['native_choices']['items']['properties']['islands']['items']
    assert 'context' in schema['required'] and schema['properties']['context']['maxItems'] == 2
