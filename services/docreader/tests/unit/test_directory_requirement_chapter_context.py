"""Requirement assignment sees the real duties behind short appendix names."""

from copy import deepcopy

from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.directory_compile import compile_directory, reading_payload, requirement_blocks
from test_directory_compile import docs, fixture_titles, item, node, plan, window


async def capture(monkeypatch):
    documents = docs(['附件甲\n制造商认证证书原件：________',
                      '附件乙\n企业失信查询截图：________',
                      '\n'.join(['技术要求', *[f'技术条款{index}' for index in range(30)]]),
                      '证书须覆盖本次产品。\n仅制造商适用。'])
    chosen = [{**node('certificate', 0, 0), 'response_kind': 'form', 'body_windows': [window(0, 0, 1)]},
              {**node('credit', 1, 0), 'response_kind': 'attachment', 'body_windows': [window(1, 0, 1)]},
              {**node('technical', 2, 0), 'response_kind': 'narrative', 'body_windows': [window(2, 0, 30)]}]
    compiled = compile_directory(plan(chosen, [
        item('certificate', clause=[window(3, 0)], condition=window(3, 1)),
        item('credit'), item('technical'),
    ]), documents)
    original = deepcopy(compiled)
    calls = []
    monkeypatch.setattr(pipeline, 'requirement_blocks', lambda value: requirement_blocks(value, limit=5))

    async def ask(_execute, _budget, task, payload, validate):
        calls.append((str(task), deepcopy(payload)))
        response = ({'chapters': []} if task == 'directory_parameter' else
                    {'assignments': [], 'table_headers': [], 'chapter_titles': fixture_titles(payload, {
                        '附件甲': '制造商认证证书', '附件乙': '企业信用查询'}), 'directory_adjudications': [
                        {'case_id': case['case_id'], 'action': 'conditional', 'basis_windows': [window(3, 1)]}
                        for case in payload.get('directory_cases', [])]} if task == 'directory_material' else
                    {'assignments': []} if task == 'directory_requirement' else {'items': []})
        return validate(response)

    monkeypatch.setattr(pipeline, '_ask', ask)
    await pipeline._request_directory_details(compiled, documents, None, None)
    return documents, original, compiled, calls


async def test_every_requirement_block_sees_complete_original_form_and_attachment_duties(monkeypatch):
    documents, original, compiled, calls = await capture(monkeypatch)
    requirements = [payload for task, payload in calls if task == 'directory_requirement']
    assert len(requirements) > 1
    for payload in requirements:
        chapters = {chapter['id']: chapter for chapter in payload['chapters']}
        assert chapters['certificate'].get('source_body') == documents[0]['pages'][0]
        assert chapters['credit'].get('source_body') == documents[0]['pages'][1]
        assert chapters['certificate'].get('source_obligations') == '证书须覆盖本次产品。'
        assert payload['applicability']['conditions'] == [
            {'chapter_id': 'certificate', 'text': '仅制造商适用。'}]
    for before, after in zip(original['chapters'], compiled['chapters']):
        assert all(after[key] == value for key, value in before.items())


async def test_role_context_does_not_expand_material_classification_or_long_narrative_inputs(monkeypatch):
    documents, _, _, calls = await capture(monkeypatch)
    material = next(payload for task, payload in calls if task == 'directory_material')
    assert all(set(chapter) == {'id', 'title', 'parent_id', 'response_kind', 'subjects'}
               for chapter in material['chapters'])
    classification = next(payload for task, payload in calls if task == 'directory_classification')
    from test_directory_source_closure_stage import restore_reading_pairs
    assert restore_reading_pairs(classification) == reading_payload(documents)
    assert set(classification) == {'documents', 'source_line_format'}
    for task, payload in calls:
        if task == 'directory_requirement':
            technical = next(chapter for chapter in payload['chapters'] if chapter['id'] == 'technical')
            assert 'source_body' not in technical
            assert 'source_obligations' not in technical
    parameter = next(payload for task, payload in calls if task == 'directory_parameter')
    assert len(parameter['chapter']['lines']) == 31
