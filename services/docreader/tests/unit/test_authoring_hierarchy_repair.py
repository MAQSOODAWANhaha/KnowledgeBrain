"""Last automatic structure review can apply a narrow, explicit hierarchy correction."""
from io import BytesIO

from docx import Document
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_pipeline import generate_document

from test_authoring_pipeline_boundaries import budget, ordinary_response, outline, payload, snapshot


@pytest.mark.parametrize('titles', [('设备组成', '补充事项'), ('实施职责', '服务范围')])
async def test_final_automatic_review_moves_the_subtree_without_a_second_assembly(tmp_path, titles):
    artifacts, calls, reviews, adopted = ArtifactStore(tmp_path), [], [], []

    async def execute(task, request, validate):
        value = payload(request)
        calls.append(task)
        if task == 'authoring_structure':
            reviews.append(value)
            if len(reviews) == 1:
                assert not value.get('hierarchy_edits_allowed', False)
                return validate({'findings': [{'code': 'wrong_hierarchy',
                    'detail': '原文明确第二项属于第一项的内部组成。',
                    'suggestion': '核对两项的父子关系。'}]})
            assert value.get('hierarchy_edits_allowed') is True
            parent, child = value['chapters']
            return validate({'findings': [], 'chapter_moves': [{
                'chapter_id': child['chapter_id'], 'parent_chapter_id': parent['chapter_id'],
                'after_chapter_id': None, 'reason': '实际原文将第二项列为第一项的子项。'}]})
        if task == 'outline' and value.get('operation') != 'assemble':
            return validate(outline(value, chapters=2))
        response = ordinary_response(task, value)
        if task == 'outline':
            response['chapters'][0]['title'] = titles[0]
            response['chapters'][1]['title'] = titles[1]
            if value.get('review_findings'):
                # The bounded full-tree revision still leaves the child promoted.
                response['chapters'][0]['title'] += '与安排'
        return validate(response)

    async def published(reference):
        adopted.append(artifacts.read_json(reference))

    result = artifacts.read_json(await generate_document(
        artifacts, snapshot(artifacts), budget(), execute, on_outline=published))
    final = artifacts.read_json(result['outline_ref'])
    assert adopted == [final]
    assert len(final['chapters']) == 1
    parent = final['chapters'][0]
    assert parent['chapter_id'] == 'chapter-1' and parent['title'] == titles[0] + '与安排'
    child, = parent['children']
    assert child['chapter_id'] == 'chapter-2' and child['title'] == titles[1]
    assert parent['targets'][0]['target_id'] == 'chapter-1:target-1'
    assert child['targets'][0]['target_id'] == 'chapter-2:target-1'
    assert len(result['unit_refs']) == 2
    doc = Document(BytesIO(artifacts.read_bytes(result['docx_ref'])))
    headings = [p.text for p in doc.paragraphs if p.style.name.startswith('Heading')]
    assert titles[1] in headings
    assert '2 ' + titles[1] not in headings
    assert calls.count('outline') == 3
    assert calls.count('authoring_structure') == 2
