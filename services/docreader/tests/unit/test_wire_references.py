"""Short references preserve literal source data and request identity."""
from copy import deepcopy
import json

from docreader.llm.budget import RequestBudget
from docreader.llm.wire import WirePayload


def payload():
    return {'schema_version': 2, 'run_id': 'run-a', 'work_unit_id': 'pack-long',
            'source_views': [{'pack_id': 'pack-long', 'document_id': 'main-doc', 'role': 'main',
                'body': [{'range_id': 'fragment-long', 'source_range_id': 'parent-long',
                          'start': 10, 'end': 15, 'section_id': 'section-long',
                          'cells': [{'column': 0, 'text': 'fragment-long 必须原样保留'}]}],
                'supporting': [], 'source_aliases': [{'range_id': 'alias-long',
                    'segments': [{'range_id': 'fragment-long', 'start': 0, 'end': 5}]}]}],
            'items': [{'item_id': 'old-pack:i3', 'text': 'fragment-long',
                       'evidence': [{'range_id': 'fragment-long', 'quote': 'fragment-long'}]}]}


def test_literal_cells_aliases_and_reference_roundtrip():
    original = payload()
    before = deepcopy(original)
    wire = WirePayload('extract', original)
    body = wire.payload['source_views'][0]['body'][0]
    assert body['range_id'] == 'r1'
    assert 'source_range_id' not in body and 'start' not in body
    assert body['cells'] == original['source_views'][0]['body'][0]['cells']
    assert wire.payload['items'][0]['text'] == 'fragment-long'
    assert wire.payload['source_views'][0]['source_aliases'][0]['segments'] == [
        {'range_id': 'r1', 'start': 0, 'end': 5}]
    restored = wire.decode({'items': [{'item_id': 'w1:i1', 'evidence': [{'range_id': 'r1', 'quote': 'r1'}]}],
                            'relations': [{'relation_id': 'w1:l1', 'from_item_id': 'i1', 'to_item_id': 'w1:i1'}]})
    assert restored['items'][0]['item_id'] == 'pack-long:i1'
    assert restored['items'][0]['evidence'] == [{'range_id': 'fragment-long', 'quote': 'r1'}]
    assert restored['relations'][0]['from_item_id'] == 'old-pack:i3'
    assert original == before


def test_unknown_permanent_and_out_of_namespace_ids_are_not_accepted():
    wire = WirePayload('verify', payload())
    for value in ('parent-long', 'fragment-long', 'r999', 'w2:i1'):
        assert wire.decode({'range_id': value})['range_id'].startswith('!unbound:')


def test_same_short_content_in_different_work_units_has_different_paid_digest():
    first = payload()
    second = {**first, 'work_unit_id': 'another-pack'}
    assert WirePayload('extract', first).payload != WirePayload('extract', second).payload


def test_budget_measures_actual_short_request_and_full_correction():
    data = payload()
    budget = RequestBudget(prompts={'extract': 'Return JSON'}, output_reserve=0)
    request = budget.request('extract', data)
    assert json.loads(request['messages'][1]['content'][0]['text']) == WirePayload('extract', data).payload
    corrected = {**data, 'contract_feedback': {'errors': [{'path': ['items'], 'code': 'invalid', 'detail': 'fix'}],
                                             'fragments': [{'path': ['items'], 'value': data['items']}]}}
    assert budget.measure('extract', corrected)['input_tokens'] > budget.measure('extract', data)['input_tokens']


def test_pending_scope_metadata_and_bindings_keep_their_semantic_bounds():
    data = payload()
    view = data['source_views'][0]
    binding = {'status': 'pending', 'kind': 'condition', 'complete_sentence': False,
               'applies_to_range_ids': ['fragment-long'], 'reviewed_target_range_ids': []}
    view['pending_scope_context'] = [{'range_id': 'pending-long', 'source_range_id': 'parent-long',
        'start': 0, 'end': 10, 'reason': '条件尚未完整', 'scope_binding': deepcopy(binding)}]
    view['scope_bindings'] = [{'range_id': 'fragment-long', 'source_range_id': 'parent-long',
        'start': 10, 'end': 15, 'scope_binding': deepcopy(binding)}]
    wire = WirePayload('verify', data)
    sent = wire.payload['source_views'][0]
    assert 'parent-long' not in json.dumps(sent)
    assert sent['pending_scope_context'][0]['reason'] == '条件尚未完整'
    for field in ('pending_scope_context', 'scope_bindings'):
        actual = sent[field][0]['scope_binding']
        assert actual == {**binding, 'applies_to_range_ids': ['r1']}


def test_unheaded_catalog_and_shared_section_labels_keep_document_identity():
    data = {**payload(), 'catalog': [
        {'document_id': 'main-doc', 'role': 'main', 'sections': [{'section_id': None, 'heading_path': ''}]},
        {'document_id': 'annex-doc', 'role': 'attachment', 'sections': [{'section_id': None, 'heading_path': ''}]}]}
    wire = WirePayload('verify', data)
    documents = wire.payload['catalog']
    assert documents[0]['document_id'] != documents[1]['document_id']
    restored = wire.decode({'section_refs': [{'document_id': documents[1]['document_id'], 'section_id': None}]})
    assert restored == {'section_refs': [{'document_id': 'annex-doc', 'section_id': None}]}


def test_search_intents_resolve_mixed_rule_and_source_refs_without_registering_unknown_ids():
    wire = WirePayload('outline', {'schema_version': 2,
        'rules': [{'rule_id': 'long-rule'}], 'sources': [{'range_id': 'long-range'}],
        'search_intents': [{'query': '原样保留 e1/r1', 'for_ids': ['long-rule', 'long-range']}]})
    assert wire.payload['search_intents'] == [{'query': '原样保留 e1/r1', 'for_ids': ['e1', 'r1']}]
    assert wire.decode({'search_intents': [{'query': '角色权限', 'for_ids': ['e1', 'r1']}]}) == {
        'search_intents': [{'query': '角色权限', 'for_ids': ['long-rule', 'long-range']}]}
    for identity in ('long-rule', 'long-range', 'e2', 'r2', 'i1'):
        assert wire.decode({'search_intents': [{'query': '角色权限', 'for_ids': [identity]}]})[
            'search_intents'][0]['for_ids'] == ['!unbound:' + identity]
