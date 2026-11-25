"""A chapter's own service duties still need responses when it has other children."""

from typing import TypeVar
from copy import deepcopy
from io import BytesIO

from docx import Document

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.directory_compile import compile_directory, outline_from_compiled
from test_authoring_pipeline_boundaries import budget, snapshot
from test_directory_compile import docs, fixture_titles, item, node, plan, window

_T = TypeVar("_T")


def _required(value: _T | None) -> _T:
    assert value is not None
    return value



async def service_case(monkeypatch, line_count=30):
    lines = ['项目服务方案', '口令专项清查', '每季度至少一次，发现问题后4小时内提交清单。',
             *[f'口令清查范围{number}' for number in range(12)], '高危端口专项清查',
             '每月一次，发现问题后12小时内复核并保留证明。',
             *[f'端口清查范围{number}' for number in range(13)]]
    assert len(lines) == 30
    documents = docs(['\n'.join(lines[:line_count]), '培训安排\n培训内容另行填写：________'])
    chosen = [{**node('service', 0, 0), 'response_kind': 'narrative',
               'body_windows': [window(0, 0, line_count - 1)]},
              {**node('training', 1, 0, parent='service'), 'response_kind': 'form',
               'body_windows': [window(1, 0, 1)]}]
    compiled = compile_directory(plan(chosen, [item('service', 'subtree')]), documents)
    original = deepcopy(compiled)
    calls = []

    async def ask(_execute, _budget, task, payload, validate):
        calls.append((str(task), deepcopy(payload)))
        if task == 'directory_material':
            response = {'assignments': [], 'table_headers': [], 'chapter_titles': fixture_titles(payload)}
        elif task == 'directory_requirement':
            response = {'assignments': []}
        elif task == 'directory_classification':
            response = {'items': []}
        else:
            assert task == 'directory_parameter'
            assert payload['chapter']['id'] == 'service'
            response = {'chapters': [{'chapter_id': 'service', 'items': [
                {**window(0, 1, 14), 'content_start_line': 1, 'title': '口令专项清查'},
                {**window(0, 15, 29), 'content_start_line': 15, 'title': '高危端口专项清查'},
            ]}]}
        return validate(response)

    monkeypatch.setattr(pipeline, '_ask', ask)
    await pipeline._request_directory_details(compiled, documents, None, None)
    return documents, original, compiled, calls


async def test_long_parent_with_an_existing_form_child_is_expanded_once_from_its_own_source(monkeypatch):
    documents, original, compiled, calls = await service_case(monkeypatch)
    requests = [payload for task, payload in calls if task == 'directory_parameter']
    assert len(requests) == 1
    assert [row['text'] for row in requests[0]['chapter']['lines']] == documents[0]['pages'][0].splitlines()
    assert all(row['page'] == 0 for row in requests[0]['chapter']['lines'])
    assert all(compiled['chapters'][1][key] == value for key, value in original['chapters'][1].items())
    parameters = [chapter for chapter in compiled['chapters'] if chapter.get('parameter_parent_id') == 'service']
    assert [chapter['display_title'] for chapter in parameters] == ['口令专项清查', '高危端口专项清查']
    assert '4小时' in parameters[0]['text'] and '12小时' in parameters[1]['text']
    assert not any('rejected' in issue['code'] for issue in compiled['issues'])


async def test_short_parent_keeps_its_existing_children_without_requesting_parameter_expansion(monkeypatch):
    _, original, compiled, calls = await service_case(monkeypatch, line_count=24)
    assert not [task for task, _ in calls if task == 'directory_parameter']
    assert len(compiled['chapters']) == len(original['chapters'])
    assert all(after[key] == value for before, after in zip(original['chapters'], compiled['chapters'])
               for key, value in before.items())


async def test_nonleaf_services_reach_word_with_separate_blank_responses_and_the_existing_form(monkeypatch, tmp_path):
    documents, _, compiled, _ = await service_case(monkeypatch)
    monkeypatch.undo()
    artifacts = ArtifactStore(tmp_path / 'artifacts')
    fixed = snapshot(artifacts, [{'text': page} for page in documents[0]['pages']])
    outline = outline_from_compiled(fixed, compiled)
    children = outline['chapters'][0]['children']
    assert len(children) == 3
    assert children[0]['targets'][0]['purpose'] == documents[0]['pages'][1]
    assert children[0]['targets'][0]['kind'] == 'form'

    async def model(task, _request, validate):
        assert task == 'authoring_structure'
        return validate({'findings': []})

    result = artifacts.read_json(await pipeline.generate_document(
        artifacts, fixed, budget(), model, outline=outline))
    document = Document(BytesIO(artifacts.read_bytes(result['docx_ref'])))
    paragraphs = document.paragraphs
    for child in children[1:]:
        start = next(index for index, paragraph in enumerate(paragraphs)
                     if _required(_required(paragraph.style).name).startswith('Heading') and paragraph.text == child['title'])
        end = next((index for index in range(start + 1, len(paragraphs))
                    if _required(_required(paragraphs[index].style).name).startswith('Heading')), len(paragraphs))
        text = '\n'.join(paragraph.text for paragraph in paragraphs[start:end])
        assert child['targets'][0]['purpose'] in text
        assert text.count('我方响应：________（人工填写）') == 1
        assert text.count('证明材料或附件位置：________（原条款要求时填写）') == 1
    assert '培训内容另行填写：________' in '\n'.join(paragraph.text for paragraph in paragraphs)
