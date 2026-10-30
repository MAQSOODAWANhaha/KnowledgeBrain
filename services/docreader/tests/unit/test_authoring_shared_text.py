"""Shared input prose preserves every byte and independent source identity."""
from copy import deepcopy
import json

from docreader.tender.authoring_view import compact_assembly_payload


def source_payload(parts, excerpts=(), submission=()):
    return {'chapters': [], 'template_parts': parts, 'source_excerpts': list(excerpts),
            'submission_sources': list(submission), 'submission_rules': [], 'unassigned_rules': []}


def restore_text(entry, view):
    if 'text_lines' not in entry:
        return entry.get('text')
    return '\n'.join(view['text_pool'][line] if type(line) is int else line
                     for line in entry['text_lines'])


def test_overlapping_multiline_sources_share_prose_without_merging_their_identities():
    first = '申请主体需独立填写并保留本项的适用条件、原始标点和全部空白。' * 8
    second = '生产主体需另外提供证明，不表示两种主体的义务可以互相替代。' * 8
    original = source_payload([
        {'part_id': 'p1', 'text': first}, {'part_id': 'p2', 'text': second}],
        [{'range_id': 'x1', 'text': first + '\n' + second},
         {'range_id': 'x2', 'text': second, 'section_id': 'different-scope'}],
        [{'source_id': 's1', 'text': first}])
    before = deepcopy(original)

    view = compact_assembly_payload(original)

    assert original == before
    assert 'text_pool' in view
    assert len(json.dumps(view, ensure_ascii=False).encode()) < len(json.dumps(original, ensure_ascii=False).encode())
    for part in original['template_parts']:
        assert restore_text(view['template_parts'][part['part_id']], view) == part['text']
    excerpts = view['source_excerpts']
    by_id = excerpts if isinstance(excerpts, dict) else {entry['range_id']: entry for entry in excerpts}
    assert list(by_id) == ['x1', 'x2']
    for entry in original['source_excerpts']:
        assert restore_text(by_id[entry['range_id']], view) == entry['text']
    assert by_id['x2']['section_id'] == 'different-scope'
    assert restore_text(view['submission_sources']['s1'], view) == first


def test_shared_lines_preserve_empty_lines_crlf_unicode_spaces_and_numeric_prose():
    line = '　完整格式：ＡＢＣ e\u0301  123；不要改变\t空白或标点。' * 8 + '\r'
    text = '\n' + line + '\n\n123\n' + line + '\n'
    view = compact_assembly_payload(source_payload([
        {'part_id': 'p1', 'text': text}, {'part_id': 'p2', 'text': line}]))
    assert 'text_pool' in view
    assert restore_text(view['template_parts']['p1'], view) == text
    assert restore_text(view['template_parts']['p2'], view) == line
    assert all(isinstance(entry, str) for entry in view['text_pool'])
    assert '123' in view['template_parts']['p1']['text_lines']


def test_unique_small_and_unavailable_text_remain_inline_without_an_unused_pool():
    original = source_payload([
        {'part_id': 'p1', 'text': '一'}, {'part_id': 'p2', 'text': '一'},
        {'part_id': 'p3', 'text': '唯一的正文。'}, {'part_id': 'p4', 'unavailable': True}])
    view = compact_assembly_payload(original)
    assert 'text_pool' not in view
    for entry in original['template_parts']:
        actual = view['template_parts'][entry['part_id']]
        assert actual == {key: value for key, value in entry.items() if key != 'part_id'}


def test_similar_but_different_prose_is_not_normalized_or_shared():
    texts = ['报价  不超过１００。' * 12, '报价 不超过100。' * 12,
             '报价\t不超过１００。' * 12, '报价\r不超过１００。' * 12]
    view = compact_assembly_payload(source_payload([
        {'part_id': f'p{i}', 'text': text} for i, text in enumerate(texts)]))
    assert 'text_pool' not in view
    assert [view['template_parts'][f'p{i}']['text'] for i in range(len(texts))] == texts


def test_single_shared_excerpt_section_is_retained_once_without_changing_range_order():
    original = source_payload([], [
        {'range_id': 'x1', 'text': '第一份原文。', 'section_id': 'section-a'},
        {'range_id': 'x2', 'text': '第二份原文。', 'section_id': 'section-a'}])
    original['source_sections'] = [{'section_id': 'section-a', 'heading_path': '完整 > 原生标题'}]
    view = compact_assembly_payload(original)
    assert view['source_sections'] == original['source_sections']
    assert list(view['source_excerpts']) == ['x1', 'x2']
    assert all('section_id' not in entry for entry in view['source_excerpts'].values())
    assert [{'range_id': identity, 'section_id': view['source_sections'][0]['section_id'], **entry}
            for identity, entry in view['source_excerpts'].items()] == original['source_excerpts']


def test_multiple_or_inconsistent_excerpt_sections_remain_explicit():
    original = source_payload([], [
        {'range_id': 'x1', 'text': '甲条件。', 'section_id': 'section-a'},
        {'range_id': 'x2', 'text': '乙条件。', 'section_id': 'section-b'}])
    original['source_sections'] = [{'section_id': 'section-a', 'heading_path': '甲'}]
    for additional in ([], [{'section_id': 'section-b', 'heading_path': '乙'}]):
        sample = deepcopy(original)
        sample['source_sections'].extend(additional)
        view = compact_assembly_payload(sample)
        assert [entry['section_id'] for entry in view['source_excerpts'].values()] == ['section-a', 'section-b']
