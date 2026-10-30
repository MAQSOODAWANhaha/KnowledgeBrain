"""Review generated content and missed knowledge, not empty form fill slots."""

from copy import deepcopy

import pytest

from docreader.tender.authoring_pipeline import _review_record


@pytest.mark.parametrize('kind,knowledge,text,reviewed', [
    ('form', False, None, False),
    ('form', False, '', False),
    ('narrative', False, None, True),
    ('form', True, None, True),
    ('form', False, 'Source-backed actual content', True),
])
def test_empty_form_review_preserves_actual_content_and_missed_match_checks(kind, knowledge, text, reviewed):
    source = {'source_index_ref': {'sha256': 'a' * 64}, 'unit_id': 'source'}
    task = {'task_id': 'task', 'title_path': ['Section'],
            'targets': [{'target_id': 'formal', 'kind': kind}]}
    unit = {'task_id': 'task', 'status': 'placeholder', 'blocks': [], 'cell_fills': [],
            'row_fills': [], 'attachments': [], 'gaps': ['Enterprise fields remain blank.']}
    if text is not None:
        unit['blocks'] = [{'kind': 'paragraph', 'text': text, 'headers': [], 'rows': [],
                           'target_ids': ['formal'], 'material_ids': [], 'source_refs': [source]}]
    supports = [{'support_id': 's1', 'origin': 'tender', 'text': 'Fixed source form'}]
    materials = [{'material_id': 'material', 'text': 'Knowledge candidate'}] if knowledge else []
    if knowledge:
        supports.append({'support_id': 'k1', 'origin': 'knowledge', 'text': 'Knowledge candidate'})
    payload = {'targets': [{'target_id': 't1'}], 'rules': [], 'supports': supports,
               'basis': {}, **{field: [] for field in ('analysis_issues', 'issue_resolutions',
                   'relations', 'unverified_relation_ids', 'related_rules')}}
    before = deepcopy((task, unit, payload, materials))

    record = _review_record(task, unit, payload, {'s1': source}, materials)

    assert (record is not None) is reviewed
    if reviewed:
        assert record['task_id'] == 'task'
        assert record['supports'] == supports
        assert record['content']['blocks'] == ([{'kind': 'paragraph', 'text': text,
            'headers': [], 'rows': [], 'content_id': 'b1', 'target_ids': ['t1'],
            'support_ids': ['s1']}] if text is not None else [])
    assert (task, unit, payload, materials) == before
