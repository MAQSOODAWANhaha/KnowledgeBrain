"""Knowledge answers sit beside read-only tender text and do not search ineligible positions."""

from docreader.parser.continuation import mark_layout_decoration
from docreader.tender.authoring_coverage import (
    attach_knowledge_answer_slots, blank_knowledge_answer, build_ledger,
    exclude_tender_materials, plan_queries,
)
from docreader.tender.directory_compile import compile_directory, outline_from_compiled, reading_payload
from test_directory_compile import docs, item, node, plan


def _outline():
    return {'chapters': [{'chapter_id': 'chapter-1', 'title': '资格', 'children': [], 'targets': [
        {'target_id': 'clause', 'kind': 'narrative', 'purpose': '须提供项目经理身份证',
         'rule_refs': [], 'source_refs': [], 'manual_fill': True, 'support_origins': ['tender'],
         'enterprise_response': True},
        {'target_id': 'pending', 'kind': 'narrative', 'purpose': '未处置的页眉',
         'rule_refs': [], 'source_refs': [], 'manual_fill': False, 'support_origins': ['knowledge'],
         'pending_line': True},
        {'target_id': 'image', 'kind': 'narrative', 'purpose': '未读图片',
         'rule_refs': [], 'source_refs': [], 'manual_fill': False, 'support_origins': ['knowledge'],
         'unread_image': True},
        {'target_id': 'attachment', 'kind': 'narrative', 'purpose': '内嵌件适用性不明',
         'rule_refs': [], 'source_refs': [], 'manual_fill': False, 'support_origins': ['knowledge'],
         'attachment_applicability_pending': True},
    ]}]}


def test_answer_slot_is_separate_and_queries_only_the_clause():
    outline = attach_knowledge_answer_slots(_outline())
    tender, answer = outline['chapters'][0]['targets'][0], outline['chapters'][0]['targets'][-1]
    assert tender['support_origins'] == ['tender'] and tender['manual_fill'] is True
    assert tender['purpose'] == '须提供项目经理身份证'
    assert 'enterprise_response' not in tender
    assert answer['target_id'] == 'clause:answer'
    assert answer['support_origins'] == ['knowledge'] and answer['manual_fill'] is False
    assert answer['purpose'] == '须提供项目经理身份证'
    assert answer['related_target_ids'] == ['clause']
    tasks = [{'task_id': 'w1', 'chapter_id': 'chapter-1', 'title_path': ['资格'],
              'targets': outline['chapters'][0]['targets']}]
    entries = build_ledger(outline, {})
    tender_hash = 'c' * 64
    planned = plan_queries(outline, tasks, entries, {}, {}, {'exclude_content_hashes': [tender_hash]})
    assert [item['query'] for item in planned['search']] == ['须提供项目经理身份证']
    assert all(item['exclude_content_hashes'] == [tender_hash] for item in planned['search'])
    assert {item['target_id'] for item in planned['search']} == {'clause:answer'}
    materials = exclude_tender_materials(
        [{'text': '历史招标原文', 'content_sha256': tender_hash}, {'text': '身份证样例', 'content_sha256': 'd' * 64}],
        [tender_hash])
    assert [item['text'] for item in materials] == ['身份证样例']
    blank_knowledge_answer(tender, answer)
    assert answer['text'] == ''
    assert tender['purpose'] == '须提供项目经理身份证'


async def test_generate_document_leaves_the_answer_blank_when_knowledge_is_the_tender(tmp_path):
    from docreader.integrations.artifacts import ArtifactStore
    from docreader.llm.budget import RequestBudget
    from docreader.tender.authoring_pipeline import generate_document
    from docreader.tender.schema import AuthoringInput

    artifacts = ArtifactStore(tmp_path)
    tender_sha = 'c' * 64
    original = {'sha256': tender_sha, 'size': 4, 'media_type': 'application/pdf', 'relative_path': 'cc/' + tender_sha}
    index = artifacts.put_json({'schema_version': 1, 'ranges': []})
    frozen = artifacts.put_json({'schema_version': 2, 'documents': [
        {'document_id': 'source-pdf', 'role': 'main', 'original_ref': original, 'source_index_ref': index}], 'issues': []})
    snap = AuthoringInput.model_validate({
        'schema_version': 2, 'run_id': 'slot-run', 'source_version': 1, 'domain_revision': 1,
        'frozen_ref': frozen}).model_dump()
    outline = {'schema_version': 2, 'authoring_input': snap, 'issues': [], 'chapters': [{
        'chapter_id': 'c1', 'title': '资格', 'children': [], 'targets': [
            {'target_id': 'clause', 'kind': 'narrative', 'purpose': '须提供项目经理身份证',
             'rule_refs': [], 'source_refs': [], 'manual_fill': True, 'support_origins': ['tender']},
            {'target_id': 'clause:answer', 'kind': 'narrative', 'purpose': '须提供项目经理身份证',
             'rule_refs': [], 'source_refs': [], 'manual_fill': False, 'support_origins': ['knowledge'],
             'related_target_ids': ['clause']},
        ]}]}

    class FakeKnowledge:
        def spec(self):
            return {'provider': 'fake'}

        async def search(self, _search, _artifacts):
            return {'materials': [{
                'material_id': 'knowledge-fragment', 'title': '招标片段', 'text': '我方已提供项目经理身份证',
                'content_sha256': 'f' * 64,
                'locator': {'document_id': 'kb-other-id', 'source_sha256': tender_sha},
            }], 'issues': []}

    async def execute(task, request, validate):
        if task == 'authoring_write':
            raise AssertionError('an empty knowledge answer must not call the model')
        return validate({'findings': []})

    result = artifacts.read_json(await generate_document(
        artifacts, snap, RequestBudget(model='fixture', context_tokens=131072, prompts={}, output_reserve=1024),
        execute, outline=outline, knowledge=FakeKnowledge()))
    units = [artifacts.read_json(ref) for ref in result['unit_refs']]
    answer = next(unit for unit in units if 'clause:answer' in unit['target_ids'])
    assert answer['blocks'][0]['text'] == ''
    saved = artifacts.read_json(result['outline_ref'])
    tender = next(target for target in saved['chapters'][0]['targets'] if target['target_id'] == 'clause')
    assert tender['purpose'] == '须提供项目经理身份证'


def test_compiled_requirement_keeps_a_tender_slot_and_a_knowledge_slot():
    compiled = compile_directory(plan([node('qual', 0, 0)], [item('qual')]), docs(['资格\n须提供项目经理身份证\n']))
    compiled['chapters'][0]['requirements'] = ['须提供项目经理身份证']
    snapshot = {'schema_version': 2, 'run_id': 'slot-test', 'source_version': 1, 'domain_revision': 1}
    outline = outline_from_compiled(snapshot, compiled)
    targets = outline['chapters'][0]['targets']
    tender = next(target for target in targets if target['purpose'] == '须提供项目经理身份证' and target['manual_fill'])
    answer = next(target for target in targets if target['target_id'] == tender['target_id'] + ':answer')
    assert tender['support_origins'] == ['tender']
    assert answer['support_origins'] == ['knowledge'] and answer['manual_fill'] is False
    assert answer['purpose'] == tender['purpose']
    assert '我方已' not in answer['purpose']


def test_directory_lines_stay_byte_identical_when_decoration_is_marked():
    pages = ['页眉\n正文甲\n', '页眉\n正文乙\n', '页眉\n正文丙\n', '页眉\n正文丁\n']
    before = ''.join(pages).encode()
    flags = mark_layout_decoration(list(pages))
    payload = reading_payload([{'document_id': 'tender', 'role': 'main', 'pages': pages, 'layout_decoration': flags}])
    lines = [line['text'] for document in payload['documents'] for page in document['pages'] for line in page['lines']]
    assert lines == [line for page in pages for line in page.splitlines()]
    assert ''.join(pages).encode() == before
