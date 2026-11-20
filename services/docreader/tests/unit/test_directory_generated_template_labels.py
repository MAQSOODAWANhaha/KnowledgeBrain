from copy import deepcopy

import pytest

from docreader.tender.directory_compile import _outline_chapter
from docreader.tender.source_presentation import collect_source_renderings


PART = {'source_ref': {'source_index_ref': {'sha256': '1' * 64}, 'unit_id': 'original-grid'}, 'selectors': []}


def chapter():
    return {'id': 'source-node', 'title': '投标响应表', 'parent_id': None,
            'text': '', 'obligations': '', 'presentation': 'body', 'response_kind': 'form'}


@pytest.mark.parametrize('branch', ['template_segment', 'body_parts', 'material_segment'])
def test_only_actual_title_fallback_has_generated_source_receipt(branch):
    source = chapter()
    if branch == 'template_segment':
        source['template_segments'] = [{'text': '', 'part': PART}]
    if branch == 'body_parts':
        source['template_parts'] = [PART]
    if branch == 'material_segment':
        source['material_segments'] = [{'text': None, 'part': PART}]
    original = deepcopy(source)
    node = _outline_chapter(source)
    target = node['targets'][0]
    assert target['_source_generated'] is True
    assert target['purpose'] == source['title'] and target['template_refs'] == [PART]
    compiled = {}
    collect_source_renderings([node], compiled)
    assert compiled['_source_renderings'][0]['generated'] is True
    assert compiled['_source_renderings'][0]['rows'] == []
    assert '_source_generated' not in target
    assert source == original


@pytest.mark.parametrize('branch', ['template_segment', 'body_parts', 'material_segment'])
@pytest.mark.parametrize('same_as_title', [False, True])
def test_real_source_text_without_rows_is_never_reclassified_as_generated(branch, same_as_title):
    source = chapter()
    text = source['title'] if same_as_title else '本表之外还须单独提交承诺。'
    if branch == 'template_segment':
        source['template_segments'] = [{'text': text, 'part': PART}]
    if branch == 'body_parts':
        source.update(text=text, template_parts=[PART])
    if branch == 'material_segment':
        source['material_segments'] = [{'text': text, 'part': PART}]
    target = _outline_chapter(source)['targets'][0]
    assert target['purpose'] == text and not target.get('_source_generated')


def test_fallback_subject_instances_keep_public_carriers_and_unchanged_text():
    source = chapter()
    source.update(text='  ', template_parts=[PART], subjects=[{'role': '投标人'}, {'role': '制造商'}])
    targets = _outline_chapter(source)['targets']
    assert [t['subject'] for t in targets] == ['投标人', '制造商']
    assert all(t['_source_generated'] and t['template_refs'] == [PART] and t['purpose'] == source['title']
               and t['manual_fill'] and t['support_origins'] == ['tender'] for t in targets)


def test_nonempty_segment_whitespace_is_not_silently_rewritten_as_a_fallback():
    source = chapter()
    source['template_segments'] = [{'text': '   ', 'part': PART}]
    target = _outline_chapter(source)['targets'][0]
    assert target['purpose'] == '   ' and not target.get('_source_generated')
