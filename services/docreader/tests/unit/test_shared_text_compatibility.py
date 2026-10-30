"""Existing assembly wire bytes and already shared pools remain stable."""
from copy import deepcopy
import json

import pytest

from docreader.tender.authoring_view import _share_text_lines, compact_assembly_payload
from test_authoring_shared_text import restore_text, source_payload


def wire(value):
    return json.dumps(value, ensure_ascii=False, separators=(',', ':')).encode()


def test_default_assembly_projection_keeps_its_previous_exact_wire_bytes():
    first = 'Exact original source line A.' * 12
    second = 'Exact original condition line B.' * 12
    original = source_payload([
        {'part_id': 'p2', 'text': first + '\n\n' + second, 'section_id': 'section-b'},
        {'part_id': 'p1', 'text': second}],
        [{'range_id': 'x2', 'text': first, 'section_id': 'section-c'}],
        [{'source_id': 'source-a', 'text': first}])
    expected = {'chapters': [],
        'template_parts': {'p2': {'section_id': 'section-b', 'text_lines': [0, '', 1]},
                           'p1': {'text_lines': [1]}},
        'source_excerpts': {'x2': {'section_id': 'section-c', 'text_lines': [0]}},
        'submission_sources': {'source-a': {'text_lines': [0]}},
        'submission_rules': {}, 'unassigned_rules': {},
        'assembly_view': 'compact_v1', 'text_pool': [first, second]}
    assert wire(compact_assembly_payload(original)) == wire(expected)


@pytest.mark.parametrize('as_dictionary', [False, True])
def test_explicit_fields_support_lists_and_dictionaries_without_touching_other_fields(as_dictionary):
    line = 'The exact same source prose with all of its conditions.' * 12
    values = [{'support_id': 's2', 'text': line}, {'support_id': 's1', 'text': line}]
    original = {'supports': {entry['support_id']: entry for entry in values} if as_dictionary else values,
                'submission_sources': [{'source_id': 'source-a', 'text': line}],
                'source_excerpts': {'x1': {'text': line}}}
    view = deepcopy(original)
    _share_text_lines(view, fields=('supports', 'submission_sources'))
    assert 'text_pool' in view
    assert view['source_excerpts'] == original['source_excerpts']
    actual = list(view['supports'].values()) if as_dictionary else view['supports']
    assert [entry['support_id'] for entry in actual] == ['s2', 's1']
    assert all(restore_text(entry, view) == line for entry in [*actual, *view['submission_sources']])


@pytest.mark.parametrize('existing_pool', [[], ['Prior unrelated source line.'], ['Repeated exact prose.' * 20]])
def test_existing_pool_is_never_overwritten_even_when_new_lines_would_save_space(existing_pool):
    line = 'Repeated exact prose.' * 20
    original = {'text_pool': existing_pool,
        'template_parts': {'old': {'text_lines': [0]} if existing_pool else {'text_lines': []},
                           'new-a': {'text': line}, 'new-b': {'text': line}}}
    view = deepcopy(original)
    _share_text_lines(view)
    assert wire(view) == wire(original)
