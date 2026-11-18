"""Short source selections retain exact source coordinates, blanks and visibility."""

from copy import deepcopy

import pytest

from docreader.llm.request import ModelContractError


def source():
    return [{'document_id': 'main', 'role': 'main', 'pages': ['甲标题\n\n甲正文', '续行\n乙标题']},
            {'document_id': 'appendix', 'role': 'attachment', 'pages': ['甲标题\n附文']}]


def refs():
    from docreader.tender.directory_sources import SourceRefs
    return SourceRefs(source(), {'source_version': 3, 'domain_revision': 7})


def test_source_ref_reading_retains_every_page_line_and_blank_without_mutation():
    from docreader.tender.directory_sources import SourceRefs
    documents = source()
    before = deepcopy(documents)
    index = SourceRefs(documents, {'source_version': 3, 'domain_revision': 7})
    view = index.reading()
    assert view['source_ref_format'] == 'host_bound_line_refs_v1'
    assert (view['source_version'], view['domain_revision']) == (3, 7)
    assert view['documents'] == [
        {'document_id': 'main', 'role': 'main', 'pages': [
            {'page': 0, 'lines': [[0, '甲标题'], [1, ''], [2, '甲正文']]},
            {'page': 1, 'lines': [[3, '续行'], [4, '乙标题']]}]},
        {'document_id': 'appendix', 'role': 'attachment', 'pages': [
            {'page': 0, 'lines': [[5, '甲标题'], [6, '附文']]}]}]
    assert documents == before


def test_packing_does_not_renumber_refs_or_merge_same_text_in_other_sources():
    index = refs()
    whole = index.reading()
    part = index.reading({('main', 1), ('appendix', 0)})
    assert part['documents'][0]['pages'] == whole['documents'][0]['pages'][1:]
    assert part['documents'][1] == whole['documents'][1]
    assert part['source_projection_digest'] == whole['source_projection_digest']
    assert index.point(0) == {'document_id': 'main', 'page': 0, 'line': 0}
    assert index.point(5) == {'document_id': 'appendix', 'page': 0, 'line': 0}


def test_page_selection_iterable_is_read_once_and_unknown_pages_are_rejected():
    index = refs()
    expected = index.reading({('main', 1)})
    assert index.reading(point for point in [('main', 1)]) == expected
    with pytest.raises(ValueError, match='unknown physical page'):
        index.reading({('main', 99)})


def test_explicit_ref_range_expands_across_pages_and_keeps_blank_source_lines():
    index = refs()
    assert index.windows(1, 3, visible={0, 1, 2, 3, 4}) == [
        {'document_id': 'main', 'page': 0, 'start_line': 1, 'end_line': 2},
        {'document_id': 'main', 'page': 1, 'start_line': 0, 'end_line': 0}]


@pytest.mark.parametrize('start,end,visible', [
    (3, 1, {1, 2, 3}), (4, 5, {4, 5}), (0, 3, {0, 2, 3}),
    (-1, 0, {-1, 0}), (0, 99, set(range(100))), (True, 2, {0, 1, 2}),
])
def test_range_never_selects_invisible_lines_or_crosses_source_documents(start, end, visible):
    with pytest.raises(ModelContractError):
        refs().windows(start, end, visible=visible)


@pytest.mark.parametrize('reference,visible', [(99, {99}), (0, {5}), (True, {1}), (1, {1})])
def test_heading_ref_must_be_visible_known_nonempty_and_strict_integer(reference, visible):
    with pytest.raises(ModelContractError):
        refs().point(reference, visible=visible, nonempty=True)


def test_source_projection_changes_on_text_identity_role_or_native_grid_change():
    from docreader.tender.directory_sources import SourceRefs
    original = source()
    digest = SourceRefs(original).reading()['source_projection_digest']
    variants = []
    for field, value in [('document_id', 'other'), ('role', 'attachment'), ('pages', ['其他原文']),
                         ('grids', [{'unit_id': 'u1', 'page': 0, 'rows': [['原表']]}])]:
        variant = deepcopy(original)
        variant[0][field] = value
        variants.append(variant)
    assert all(SourceRefs(value).reading()['source_projection_digest'] != digest for value in variants)
