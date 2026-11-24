"""Reviewed source facts are explicit outline input and survive bounded retries."""

from copy import deepcopy

import pytest

from docreader.integrations import web
from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.item_placement import REQUIREMENT_CHAIN_CONTRACT


def reference(tmp_path):
    return ArtifactStore(tmp_path).put_json({'review': 'source-bound facts'})


def test_old_authoring_request_is_byte_compatible_when_facts_are_absent(tmp_path):
    assert web._authoring_request({}, outline_only=True) == {
        'previous_execution_id': None, 'review_request': None}
    assert web._authoring_request({'reviewed_source_facts_ref': None}, outline_only=True) == {
        'previous_execution_id': None, 'review_request': None}


def test_outline_request_pins_exact_reviewed_fact_artifact(tmp_path):
    ref = reference(tmp_path)
    actual = web._authoring_request({'reviewed_source_facts_ref': ref}, outline_only=True)
    assert actual['reviewed_source_facts_ref'] == ref


@pytest.mark.parametrize('raw', [{'sha256': 'missing-fields'}, {'media_type': 'text/plain'}])
def test_invalid_reviewed_fact_reference_is_a_safe_input_error(raw):
    with pytest.raises(web.WebError) as rejected:
        web._authoring_request({'reviewed_source_facts_ref': raw}, outline_only=True)
    assert rejected.value.code == 'INVALID_REVIEWED_SOURCE_FACTS'


def test_fixed_outline_document_generation_cannot_replace_reviewed_source_facts(tmp_path):
    with pytest.raises(web.WebError) as rejected:
        web._authoring_request({'reviewed_source_facts_ref': reference(tmp_path)}, outline_only=False)
    assert rejected.value.code == 'INVALID_OUTLINE_OPTIONS'


def test_omitted_fact_ref_inherits_from_the_same_explicit_predecessor_without_mutating_request(tmp_path):
    ref = reference(tmp_path)
    request = {'previous_execution_id': 'previous', 'review_request': None}
    before = deepcopy(request)
    effective = web._inherit_reviewed_fact_request(request, {'reviewed_source_facts_ref': ref})
    assert effective['reviewed_source_facts_ref'] == ref and request == before
    assert web._inherit_reviewed_fact_request(request, {}) == request


def test_changed_reviewed_fact_ref_is_preserved_for_successor_validation(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    first, second = [artifacts.put_json({'fact_set': i}) for i in range(2)]
    actual = web._inherit_reviewed_fact_request({'reviewed_source_facts_ref': second},
                                               {'reviewed_source_facts_ref': first})
    assert actual['reviewed_source_facts_ref'] == second


def test_automatic_outline_request_preserves_the_explicit_pilot_contract():
    request = web._authoring_request({'requirement_chain_contract': REQUIREMENT_CHAIN_CONTRACT}, outline_only=True)
    assert request['requirement_chain_contract'] == REQUIREMENT_CHAIN_CONTRACT
    assert 'requirement_chain_contract' not in web._authoring_request({}, outline_only=True)


@pytest.mark.parametrize(('contract', 'outline_only'), [('unknown', True), (REQUIREMENT_CHAIN_CONTRACT, False)])
def test_pilot_option_only_accepts_the_known_automatic_outline_contract(contract, outline_only):
    with pytest.raises(web.WebError) as rejected:
        web._authoring_request({'requirement_chain_contract': contract}, outline_only=outline_only)
    assert rejected.value.code == 'INVALID_OUTLINE_OPTIONS'


def test_pilot_option_does_not_accept_legacy_reviewed_source_facts(tmp_path):
    with pytest.raises(web.WebError) as rejected:
        web._authoring_request({'requirement_chain_contract': REQUIREMENT_CHAIN_CONTRACT,
                                'reviewed_source_facts_ref': reference(tmp_path)}, outline_only=True)
    assert rejected.value.code == 'INVALID_OUTLINE_OPTIONS'
