"""Assembly partitions retain complete evidence for every selected rule."""

from copy import deepcopy

import pytest

from docreader.tender.authoring_pipeline import _assembly_subset


def assembly_payload():
    target = {'target_id': 't1', 'kind': 'narrative', 'purpose': '逐项响应',
              'range_ids': ['x1'], 'rule_ids': ['a1'], 'template_part_ids': ['p1'],
              'response_table': {'context_range_ids': ['xc'], 'rows': [
                  {'range_ids': ['x1'], 'rule_ids': ['a2']}]}}
    return {
        'operation': 'assemble',
        'chapters': [
            {'title': '所选分册', 'targets': [], 'children': [{'title': '响应表', 'targets': [target]}]},
            {'title': '其他分册', 'targets': [{'target_id': 't2', 'rule_ids': ['a3'], 'range_ids': ['xz']}]},
        ],
        'candidate_rules': [
            {'rule_id': 'a1', 'rule': {'text': '完整义务及条件', 'conditions': ['仅限适用场景'],
                                     'evidence': [{'range_ids': ['x2'], 'quote': '义务'}]}},
            {'rule_id': 'a2', 'rule': {'text': '响应行规则', 'exceptions': ['豁免情形仍待确认'],
                                     'evidence': [{'range_ids': ['x2', 'x3', 'x2'], 'quote': '条件'}]}},
            {'rule_id': 'a3', 'rule': {'text': '未选规则',
                                     'evidence': [{'range_ids': ['xz'], 'quote': '其他'}]}},
        ],
        'unassigned_rules': [{'rule_id': 'u1', 'rule': {'text': '待安排义务',
            'evidence': [{'range_ids': ['xu', 'x2'], 'quote': '待安排'}]}}],
        'source_excerpts': [
            {'range_id': 'x2', 'text': '完整条件：仅限指定场景，保留例外及签章要求。', 'section_id': 's1'},
            {'range_id': 'x1', 'text': '响应内容的完整原文。', 'section_id': 's1'},
            {'range_id': 'x3', 'text': '完整条件：仅限指定场景，保留例外及签章要求。', 'section_id': 's2'},
            {'range_id': 'xc', 'text': '全部响应行的共同条件。', 'section_id': 's1'},
            {'range_id': 'xu', 'text': '待安排义务的完整原文及例外。', 'section_id': 's3'},
            {'range_id': 'xz', 'text': '其他分册的原文。', 'section_id': 's4'},
        ],
        'template_parts': [{'part_id': 'p1', 'grid': {'rows': [['原始条件', '']], 'merged_cells': []}}],
        'template_runs': [],
        'submission_rules': [{'rule_id': 's1', 'text': '签章要求', 'conditions': ['共同适用条件']}],
        'submission_sources': [{'source_id': 's1', 'text': '共同提交依据的完整原文。'}],
    }


@pytest.mark.parametrize('include_unassigned', [False, True])
def test_partition_closes_candidate_and_response_row_evidence_without_changing_content(include_unassigned):
    payload = assembly_payload()
    before = deepcopy(payload)

    subset = _assembly_subset(payload, payload['chapters'][:1], include_unassigned=include_unassigned)

    expected = deepcopy(payload)
    expected['chapters'] = before['chapters'][:1]
    expected['candidate_rules'] = before['candidate_rules'][:2]
    expected['unassigned_rules'] = before['unassigned_rules'] if include_unassigned else []
    expected['source_excerpts'] = before['source_excerpts'][:5 if include_unassigned else 4]
    assert subset == expected
    assert payload == before
    assert [entry['range_id'] for entry in subset['source_excerpts']] == (
        ['x2', 'x1', 'x3', 'xc', 'xu'] if include_unassigned else ['x2', 'x1', 'x3', 'xc'])


def test_partition_does_not_replace_unavailable_complete_evidence_with_quotes():
    payload = assembly_payload()
    evidence = [{'range_ids': ['unavailable'], 'quote': '只剩引文，不能冒充完整原文'}, {'quote': '没有可绑定来源'}]
    payload['candidate_rules'][0]['rule']['evidence'] = deepcopy(evidence)
    before = deepcopy(payload)

    subset = _assembly_subset(payload, payload['chapters'][:1], include_unassigned=False)

    assert subset['candidate_rules'][0]['rule']['evidence'] == evidence
    assert all(entry['range_id'] != 'unavailable' for entry in subset['source_excerpts'])
    assert all(entry['text'] not in {item['quote'] for item in evidence} for entry in subset['source_excerpts'])
    assert payload == before
