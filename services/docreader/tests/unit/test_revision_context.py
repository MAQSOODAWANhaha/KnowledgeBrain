"""A structure revision sends decision text, not every adopted form."""

from docreader.llm.budget import RequestBudget
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.authoring_view import slim_revision_payload


def test_revision_omits_adopted_bodies_that_findings_do_not_name():
    payload = {
        'template_parts': [
            {'part_id': 'p1', 'text': '投标函正文', 'grid': {'row_count': 1, 'column_count': 1, 'cells': []}},
            {'part_id': 'p2', 'text': '已采纳财务表', 'grid': {'row_count': 2, 'column_count': 1, 'cells': []}},
        ],
        'source_excerpts': [
            {'range_id': 'x1', 'text': '投标函被点名的行'},
            {'range_id': 'x2', 'text': '协同防御必须逐项响应。'},
            {'range_id': 'x3', 'text': '已经绑定且没有被点名的行'},
        ],
        'unassigned_rules': [{'rule_id': 'u1', 'rule': {'evidence': [{'range_ids': ['x2']}]}}],
        'submission_sources': [{'source_id': 's1', 'text': '提交顺序保持完整'}],
    }
    previous = [
        {'title': '函件', 'targets': [{
            'review_target_id': 'v1', 'template_part_ids': ['p1'],
            'response_table': {'context_range_ids': [], 'rows': [{'range_ids': ['x1']}]}}],
         'children': []},
        {'title': '财务', 'targets': [{
            'review_target_id': 'v2', 'template_part_ids': ['p2'],
            'response_table': {'rows': [{'range_ids': ['x3']}]}}],
         'children': []},
    ]
    slim = slim_revision_payload(payload, previous, [{'detail': '核对 v1 的页首', 'suggestion': ''}])
    parts = {part['part_id']: part for part in slim['template_parts']}
    excerpts = {item['range_id']: item for item in slim['source_excerpts']}

    assert slim['revision_view'] == 'bound_v1'
    assert parts['p1']['text'] == '投标函正文'
    assert parts['p2']['retained'] is True and 'text' not in parts['p2'] and 'grid' not in parts['p2']
    assert excerpts['x1']['text'] == '投标函被点名的行'
    assert excerpts['x2']['text'] == '协同防御必须逐项响应。'
    assert excerpts['x3']['retained'] is True and 'text' not in excerpts['x3']
    assert slim['submission_sources'][0]['text'] == '提交顺序保持完整'
    assert '已采纳财务表' not in str(slim['template_parts'])
    assert '已经绑定且没有被点名的行' not in str(slim['source_excerpts'])


def _two_chapter_payload():
    return {
        'operation': 'assemble',
        'chapters': [
            {'title': '商务', 'targets': [{'template_part_ids': ['p1'], 'range_ids': ['x1'], 'rule_ids': ['a1']}], 'children': []},
            {'title': '技术', 'targets': [{'template_part_ids': ['p2'], 'range_ids': ['x2']}], 'children': []},
        ],
        'template_parts': [{'part_id': 'p1', 'text': '商务原表'}, {'part_id': 'p2', 'text': '技术原表'}],
        'source_excerpts': [{'range_id': 'x1', 'text': '商务要求'}, {'range_id': 'x2', 'text': '技术要求'}],
        'candidate_rules': [{'rule_id': 'a1', 'rule': {'text': '商务规则'}}],
        'unassigned_rules': [{'rule_id': 'u1', 'rule': {'text': '未分配', 'evidence': [{'range_ids': ['x2']}]}}],
        'template_runs': [['p1', 'p2']],
        'submission_sources': [{'source_id': 's1', 'text': '目录'}],
        'submission_rules': [],
    }


def test_fitting_assembly_stays_one_request():
    budget = RequestBudget(model='fixture', context_tokens=131072)
    assert pipeline._assembly_batches(_two_chapter_payload(), budget) == [_two_chapter_payload()]


def test_oversized_assembly_splits_chapters_without_resending_other_bodies(monkeypatch):
    def fits(_budget, _task, payload, headroom_tokens=0):
        return len(payload.get('chapters') or []) <= 1
    monkeypatch.setattr(pipeline, '_fits', fits)
    batches = pipeline._assembly_batches(_two_chapter_payload(), RequestBudget(model='fixture', context_tokens=131072))
    assert [[chapter['title'] for chapter in batch['chapters']] for batch in batches] == [['商务'], ['技术']]
    assert batches[0]['template_parts'] == [{'part_id': 'p1', 'text': '商务原表'}]
    assert '技术原表' not in str(batches[0])
    assert batches[0]['unassigned_rules'] == []
    assert batches[1]['unassigned_rules'][0]['rule_id'] == 'u1'
    assert batches[0]['template_runs'] == [] and batches[1]['template_runs'] == []
    assert batches[0]['submission_sources'][0]['text'] == '目录'


def test_joined_batches_renumber_and_keep_candidate_bindings():
    first = ([{'title': '商务', 'children': [], 'targets': [
        {'target_id': 'old-a', 'related_target_ids': [], 'response_table': {'rows': []}}]}],
        False, [], set(), [], [], {'old-a': ['t1']}, {'old-a': {'same_instance': True}})
    second = ([{'title': '技术', 'children': [], 'targets': [
        {'target_id': 'old-b', 'related_target_ids': ['old-a'], 'response_table': {'rows': []}}]}],
        False, [], set(), [], [], {'old-b': ['t2']}, {})
    chapters, omitted, issues, origins, constraints, intents, bindings, selections = pipeline._join_assemblies([first, second])
    assert omitted is False and issues == [] and origins == set() and constraints == [] and intents == []
    assert [chapter['title'] for chapter in chapters] == ['商务', '技术']
    business, technical = chapters[0]['targets'][0], chapters[1]['targets'][0]
    assert business['target_id'] != 'old-a' and technical['related_target_ids'] == [business['target_id']]
    assert bindings[business['target_id']] == ['t1'] and bindings[technical['target_id']] == ['t2']
    assert selections[business['target_id']] == {'same_instance': True}
