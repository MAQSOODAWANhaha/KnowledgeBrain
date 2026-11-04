"""Failed global assembly preserves already accepted local chapter trees."""

from copy import deepcopy
from io import BytesIO
import json
from zipfile import ZipFile

from docx import Document

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelWorkUnitExhausted
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.authoring_export import export_authoring_document
from test_authoring_templates import create_context, create_source


def targets(chapters):
    return [target for chapter in pipeline._flatten(chapters) for target in chapter['targets']]


async def organize(tmp_path, monkeypatch, *, revise=False):
    artifacts, index_ref, native = create_source(tmp_path, [
        {'text': '封面固定正文'}, {'text': '投标人财务证明原件'}, {'text': '外部密封递交事项'},
        {'text': '原厂财务证明原件'}, {'text': '共同条件：仅适用于版本甲'}, {'text': '技术参数：支持离线部署'}])
    context = create_context(artifacts, index_ref, native)
    fragments, calls, gaps, reviews = [], [], [], []

    def batches(records, *_args, **_kwargs):
        assert len(records) == 6
        return [records[:3], records[3:]]

    monkeypatch.setattr(pipeline, '_batches', batches)

    async def execute(stage, request, validate):
        value = json.loads(request['messages'][1]['content'][0]['text'])
        calls.append(stage)
        if stage == 'authoring_structure':
            reviews.append(deepcopy(value))
            if len(reviews) > 1:
                return validate({'findings': []})
            first = targets(value['chapters'])[0]
            return validate({'findings': [{'code': 'check_instance', 'detail': '检查各主体的独立位置。'}],
                'source_decoration_exclusions': [{'target_id': first['target_id'],
                    'support_ids': first['template_support_ids'], 'reason': '本实例仅此固定来源页首应排除。'}]})
        if value.get('operation') == 'assemble':
            if 'previous_structure' not in value:
                raise ModelWorkUnitExhausted('A0 transport attempts exhausted')
            previous = targets(value['previous_structure'])
            assert [(entry['review_target_id'], entry['candidate_target_ids']) for entry in previous] == [
                ('v1', ['t1']), ('v2', ['t2']), ('v3', ['t3']), ('v4', ['t4']), ('v5', ['t5'])]
            candidates = {entry['target_id']: entry for chapter in value['chapters'] for entry in chapter['targets']}
            assert previous[0]['template_part_ids'] == []
            assert previous[0]['excluded_template_part_ids'] == candidates['t1']['template_part_ids']
            assert previous[1]['subject'] == '投标人' and previous[3]['subject'] == '原厂'

            def select(identity):
                candidate = candidates[identity]
                return {'kind': candidate['kind'], 'purpose': candidate['purpose'], 'target_ids': [identity],
                    **({'template_part_ids': candidate['template_part_ids']} if identity == 't1' else {})}

            return validate({'chapters': [
                {'title': '修订封面', 'presentation_role': 'cover', 'targets': [select('t1')]},
                {'title': '财务证明', 'targets': [select('t4'), select('t2')]},
                {'title': '技术响应', 'targets': [select('t5')]},
                {'title': '递交事项', 'presentation_role': 'submission_note', 'targets': [select('t3')]}]})
        ranges = [entry['range_id'] for entry in value['sources']]
        parts = {part['range_ids'][0]: part['part_id'] for part in value['template_parts']}

        def form(index, subject=None):
            return {'kind': 'form', 'purpose': '保全此主体原件', 'rule_ids': [],
                    'range_ids': [ranges[index]], 'template_part_ids': [parts[ranges[index]]],
                    **({'subject': subject} if subject else {})}

        if not fragments:
            chapters = [
                {'title': '投标文件封面', 'presentation_role': 'cover', 'targets': [form(0)]},
                {'title': '商务文件', 'is_volume': True, 'targets': [], 'children': [
                    {'title': '财务证明', 'targets': [form(1, '投标人')]}]},
                {'title': '电子递交待办', 'presentation_role': 'submission_note', 'targets': [], 'children': [
                    {'title': '密封交付', 'targets': [{'kind': 'narrative', 'purpose': '只用于递交待办',
                        'rule_ids': [], 'range_ids': [ranges[2]], 'support_origins': ['tender']}]}]}]
        else:
            chapters = [
                {'title': '财务证明', 'targets': [form(0, '原厂')]},
                {'title': '技术文件', 'is_volume': True, 'targets': [], 'children': [
                {'title': '技术参数响应', 'targets': [{'kind': 'form', 'purpose': '逐项响应原参数',
                    'rule_ids': [], 'range_ids': [ranges[2]], 'response_table': {
                        'context_range_ids': [ranges[1]],
                        'rows': [{'range_ids': [ranges[2]], 'rule_ids': []}]}}]}]}]
        accepted = validate({'schema_version': 2, 'findings': [], 'chapters': chapters})
        fragments.append(deepcopy(accepted))
        return accepted

    result = await pipeline._organize(artifacts, context,
        RequestBudget(model='fixture', context_tokens=131072), execute, gaps,
        structure_results={} if revise else None)
    if revise:
        assert calls == ['outline', 'outline', 'outline', 'authoring_structure', 'outline', 'authoring_structure']
        assert not gaps
    else:
        assert calls == ['outline', 'outline', 'outline']
        assert [gap['code'] for gap in gaps] == ['outline_assembly_unavailable']
    return artifacts, result, fragments


async def test_failed_assembly_keeps_nested_local_trees_and_unique_bound_identities(tmp_path, monkeypatch):
    _, result, fragments = await organize(tmp_path, monkeypatch)
    chapters = result['chapters']
    assert [chapter['title'] for chapter in chapters] == [
        '投标文件封面', '商务文件', '电子递交待办', '财务证明', '技术文件']
    assert chapters[1]['targets'] == chapters[4]['targets'] == []
    flags = {chapter['title']: chapter.get('is_volume', False) for chapter in pipeline._flatten(chapters)}
    assert flags['商务文件'] and flags['技术文件']
    assert not flags['投标文件封面']
    assert fragments[0]['chapters'][1]['is_volume'] and fragments[1]['chapters'][1]['is_volume']
    assert [chapter['title'] for chapter in chapters[4]['children']] == ['技术参数响应']
    assert [chapter['chapter_id'] for chapter in pipeline._flatten(chapters)] == [
        'chapter-1', 'chapter-2', 'chapter-2.1', 'chapter-3', 'chapter-3.1',
        'chapter-4', 'chapter-5', 'chapter-5.1']
    actual = targets(chapters)
    assert len({target['target_id'] for target in actual}) == len(actual) == 5
    assert targets(fragments[0]['chapters'])[0]['target_id'] == targets(fragments[1]['chapters'])[0]['target_id']
    assert [(target.get('subject'), target['source_refs'][0]['unit_id']) for target in actual[1:4:2]] == [
        ('投标人', 'unit:1'), ('原厂', 'unit:3')]
    expected = [target for fragment in fragments for target in targets(fragment['chapters'])]
    for retained, original in zip(actual, expected):
        for field in ('source_refs', 'source_excerpts', 'template_refs', 'rule_refs'):
            assert retained.get(field) == original.get(field)
    table = actual[-1]['response_table']
    original_table = expected[-1]['response_table']
    assert table['context_source_excerpts'] == original_table['context_source_excerpts']
    assert table['rows'][0]['source_excerpts'] == original_table['rows'][0]['source_excerpts']
    assert table['rows'][0]['rule_refs'] == original_table['rows'][0]['rule_refs']
    assert table['rows'][0]['row_id'] != original_table['rows'][0]['row_id']


async def test_failed_assembly_preserves_cover_and_excludes_submission_descendants_from_body(tmp_path, monkeypatch):
    artifacts, result, _ = await organize(tmp_path, monkeypatch)
    written = {ref['unit_id'] for task in pipeline.plan_writing_tasks(result)
               for target in task['targets'] for ref in target['source_refs']}
    assert 'unit:2' not in written
    assert 'unit:1' in written and 'unit:3' in written
    assert result['chapters'][0]['presentation_role'] == 'cover'
    exported = export_authoring_document(artifacts, result, [])
    document = Document(BytesIO(artifacts.read_bytes(exported['docx_ref'])))
    headings = [paragraph.text for paragraph in document.paragraphs if paragraph.style.name.startswith('Heading')]
    assert any('商务文件' in title for title in headings)
    assert all('密封交付' not in title and '电子递交待办' not in title for title in headings)
    assert all('投标文件封面' not in title for title in headings)
    with ZipFile(BytesIO(artifacts.read_bytes(exported['package_ref']))) as package:
        assert json.loads(package.read('manifest.json'))['volumes'] == []


async def test_fallback_candidates_survive_exact_f0_exclusion_and_a1_reordering(tmp_path, monkeypatch):
    _, result, fragments = await organize(tmp_path, monkeypatch, revise=True)
    cover, manufacturer, bidder, response, note = targets(result['chapters'])
    assert cover['template_refs'] == []
    assert cover['source_refs'] == targets(fragments[0]['chapters'])[0]['source_refs']
    assert [(target['subject'], target['source_refs'][0]['unit_id']) for target in (manufacturer, bidder)] == [
        ('原厂', 'unit:3'), ('投标人', 'unit:1')]
    assert manufacturer['template_refs'] == targets(fragments[1]['chapters'])[0]['template_refs']
    assert bidder['template_refs'] == targets(fragments[0]['chapters'])[1]['template_refs']
    assert response['response_table']['context_source_excerpts'] == targets(
        fragments[1]['chapters'])[-1]['response_table']['context_source_excerpts']
    assert note['source_refs'][0]['unit_id'] == 'unit:2'
