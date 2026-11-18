"""A reference excluded from submission cannot remain or return as chapter body."""

from copy import deepcopy

import pytest

from docreader.tender.directory_compile import DirectoryPlanRejected
from docreader.tender.directory_choices import apply_directory_adjudications, directory_cases
from test_directory_choices import promise_and_bank
from test_directory_compile import window


def choices(compiled, documents, performance='alternative'):
    return [{'case_id': case['case_id'], 'action': 'not_applicable_reference' if case['kind'] == 'extra'
             else performance, 'basis_windows': [window(0, 4)]}
            for case in directory_cases(compiled, documents)]


def excluded_reference():
    compiled, documents = promise_and_bank()
    apply_directory_adjudications(compiled, documents, choices(compiled, documents))
    return compiled


def assignment(scope='chapter', *, start=3, continuation=(), document_id='tender'):
    return {**window(0, start, document_id=document_id), 'scope': scope,
            'chapter_ids': ['promise'] if scope in {'chapter', 'chapters'} else [],
            'continuation_windows': list(continuation)}


def test_not_applicable_reference_with_its_body_still_selected_is_rejected_atomically():
    compiled, documents = promise_and_bank()
    original = deepcopy(compiled)
    with pytest.raises(DirectoryPlanRejected, match='not_applicable_reference'):
        apply_directory_adjudications(compiled, documents, choices(compiled, documents, 'current'))
    assert compiled == original


def test_reference_exclusions_are_body_only_finite_source_records_and_do_not_mutate_provenance():
    from docreader.tender.directory_choices import reference_exclusions
    compiled = excluded_reference()
    original = deepcopy(compiled)
    exclusion, = reference_exclusions(compiled)
    assert set(exclusion) == {'case_id', 'body_windows', 'basis_windows', 'basis_text'}
    assert exclusion['body_windows'] == [window(0, 3)]
    assert exclusion['basis_windows'] == [window(0, 4)]
    assert exclusion['basis_text'] == '本次不要求实际担保'
    assert exclusion['case_id'] == next(d['case_id'] for d in compiled['directory_decisions']
                                         if d['action'] == 'not_applicable_reference')
    exclusion['body_windows'][0]['start_line'] = 0
    exclusion['basis_windows'][0]['start_line'] = 0
    assert compiled == original


@pytest.mark.parametrize('unplaced', [False, True])
def test_post_material_body_guard_rejects_only_selected_body_owners(unplaced):
    from docreader.tender.directory_choices import validate_reference_exclusions
    compiled = excluded_reference()
    extra = deepcopy(compiled['chapters'][0])
    extra.update(id='added-material', unplaced=unplaced,
                 lines=[{'document_id': 'tender', 'page': 0, 'line': 3}])
    compiled['chapters'].append(extra)
    if unplaced:
        validate_reference_exclusions(compiled)
    else:
        with pytest.raises(DirectoryPlanRejected, match='added-material'):
            validate_reference_exclusions(compiled)


def test_heading_is_not_body_and_equal_text_at_other_coordinates_is_allowed():
    from docreader.tender.directory_choices import validate_reference_exclusions
    compiled = excluded_reference()
    chapter = compiled['chapters'][0]
    chapter['heading'] = {'document_id': 'tender', 'page': 0, 'line': 3}
    chapter['text'] = '银行不可撤销保证'
    chapter['lines'] = [{'document_id': 'tender', 'page': 0, 'line': 2},
                        {'document_id': 'tender', 'page': 0, 'line': 5},
                        {'document_id': 'another-document', 'page': 0, 'line': 3}]
    validate_reference_exclusions(compiled)


@pytest.mark.parametrize('scope', ['chapter', 'chapters'])
@pytest.mark.parametrize('continuation', [False, True])
def test_chapter_requirement_cannot_reintroduce_excluded_body_in_first_or_continuation_window(scope, continuation):
    from docreader.tender.directory_choices import validate_requirement_exclusions
    compiled = excluded_reference()
    value = assignment(scope, start=1, continuation=[window(0, 3)]) if continuation else assignment(scope)
    original = deepcopy(compiled)
    with pytest.raises(DirectoryPlanRejected, match='not_applicable_reference'):
        validate_requirement_exclusions(compiled, [value])
    assert compiled == original


@pytest.mark.parametrize('scope', ['unassigned', 'document', 'procedure'])
def test_reference_can_remain_in_unassigned_or_document_procedure_guidance(scope):
    from docreader.tender.directory_choices import validate_requirement_exclusions
    validate_requirement_exclusions(excluded_reference(), [assignment(scope)])


@pytest.mark.parametrize('value', [assignment(start=1), assignment(start=2), assignment(start=5),
                                    assignment(document_id='another-document')])
def test_bidder_promise_reference_heading_and_distinct_source_body_are_not_excluded(value):
    from docreader.tender.directory_choices import validate_requirement_exclusions
    validate_requirement_exclusions(excluded_reference(), [value])
