"""Explicit v3 catalog records preserve the complete legacy material contract."""
from typing import Any, cast

from copy import deepcopy
import hashlib
import json

import pytest

from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelContractError
from docreader.tender.authoring_pipeline import _parse, _request
from docreader.tender.directory_compile import MaterialPlan
from docreader.tender.directory_material_ids import MaterialIds
from test_directory_material_ids import payload, response


LEGACY = 'material_short_ids_v2'
CURRENT = 'material_short_ids_v3'
CATALOGS = ('chapters', 'table_header_catalog', 'chapter_title_catalog', 'directory_cases',
            'response_form_catalog', 'source_group_candidates', 'source_carrier_catalog',
            'reviewed_source_facts', 'unplaced_tables')


def wire(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()


def test_legacy_complete_payload_and_request_wire_goldens():
    # Fixed prompt isolates wire compatibility from intentional production prompt revisions.
    projected = MaterialIds(payload()).payload
    request = _request(RequestBudget(model='fixture', prompts={'directory_material': 'fixture material protocol'}),
                       'directory_material', projected)
    assert hashlib.sha256(wire(projected)).hexdigest() == '9e7092a3620f59a933aa4f04a771233d8bc408fa48e2b665a52d1dc75373e6a2'
    assert hashlib.sha256(wire(request)).hexdigest() == '616375c8e063bd8982f5e4106aeb35ce933e72101419236f07250297f72e6dbb'
    def ordered(value):
        return json.dumps(value, ensure_ascii=False, separators=(',', ':')).encode()
    assert hashlib.sha256(ordered(projected)).hexdigest() == '1a5874be655e153e7434423d0eb58b9b6630abaeb951d8d39cee9528860f3ef9'
    assert hashlib.sha256(ordered(request)).hexdigest() == 'd1d48a8f7c195efb0283d6a0a4d77e32c2e5a2fd5357225c28e904706645068c'
    assert projected['identity_binding'] == '8facf5554e4dd918347256eae8cd49c44753174b1a1f99d6d8a745cecc7bf049'
    assert MaterialIds(payload(), contract=LEGACY).payload == projected


@pytest.mark.parametrize('catalog', CATALOGS)
def test_v3_encodes_only_named_top_level_catalogs(catalog):
    original = payload()
    original[catalog] = [{'label': 'one', 'kind': 'same'}, {'label': 'two', 'kind': 'same'}]
    before = deepcopy(original)
    legacy = MaterialIds(original).payload
    ids = MaterialIds(original, contract=CURRENT)
    assert ids.payload[catalog] == {'blocks': [{'columns': ['label'],
        'constant_values': {'kind': 'same'}, 'rows': [['one'], ['two']]}]}
    assert ids.payload['identity_format'] == CURRENT
    assert ids.payload['catalog_record_format'] == 'column_blocks_v1'
    assert ids.payload['documents'] == legacy['documents']
    assert ids.restore_payload() == original == before


def test_contiguous_key_sets_and_key_order_keep_distinct_blocks_and_record_order():
    original = payload()
    original['reviewed_source_facts'] = [
        {'label': 'first', 'value': None}, {'label': 'second', 'value': ''},
        {'value': None, 'label': 'third'}, {'label': 'fourth', 'value': []}, {}, {}]
    ids = MaterialIds(original, contract=CURRENT)
    blocks = ids.payload['reviewed_source_facts']['blocks']
    assert [len(block['rows']) for block in blocks] == [2, 1, 1, 2]
    assert blocks[-1] == {'columns': [], 'constant_values': {}, 'rows': [[], []]}
    assert json.dumps(ids.restore_payload(), ensure_ascii=False) == json.dumps(original, ensure_ascii=False)


def test_constant_fields_keep_json_types_missing_null_empty_and_nested_values_distinct():
    original = payload()
    values = [None, '', [], {}, False, 0, True, 1, 1.0]
    original['reviewed_source_facts'] = [
        {'label': str(index), 'value': value, 'nested': {'values': [value]}, 'constant': False}
        for index, value in enumerate(values)] + [{'label': 'missing', 'constant': False}]
    ids = MaterialIds(original, contract=CURRENT)
    blocks = ids.payload['reviewed_source_facts']['blocks']
    assert blocks[0]['constant_values'] == {'constant': False}
    assert len(blocks) == 2
    assert wire(ids.restore_payload()) == wire(original)


@pytest.mark.parametrize('records', [[], [{'label': 'one'}], [{'label': 'one'}, None]])
def test_empty_singleton_or_mixed_catalogs_are_not_column_encoded(records):
    original = payload()
    original['unplaced_tables'] = records
    ids = MaterialIds(original, contract=CURRENT)
    assert ids.payload['unplaced_tables'] == records
    assert ids.restore_payload() == original


def test_documents_nested_catalogs_literal_ids_and_native_grids_are_not_rewritten_by_v3():
    original = payload()
    original['documents'][0]['chapters'] = [{'literal': 'c1'}, {'literal': 'd1'}]
    original['table_header_catalog'] *= 2
    grid = {'document_id': 'source-long-id', 'id': 'c1',
            'cells': [['c1', 'd1', 'chapter-long-id', False, 0, None]]}
    for record in original['table_header_catalog']:
        record['grid'] = deepcopy(grid)
    legacy = MaterialIds(original)
    ids = MaterialIds(original, contract=CURRENT)
    assert wire(ids.payload['documents']) == wire(legacy.payload['documents'])
    assert ids.payload['table_header_catalog']['blocks'][0]['constant_values']['grid'] == grid
    assert wire(cast(Any, ids.restore_payload())['documents']) == wire(original['documents'])
    assert ids.restore_payload() == original
    ids.payload['table_header_catalog']['blocks'][0]['constant_values']['grid']['cells'][0][0] = 'changed'
    assert original['table_header_catalog'][0]['grid'] == grid


def test_catalogs_expand_before_source_coordinates_and_permanent_identities_restore():
    original = payload()
    original['chapters'] = [
        {'id': 'chapter-long-id', 'heading': {'document_id': 'source-long-id', 'page': 0, 'line': 0}},
        {'id': 'other-chapter-long-id', 'heading': {'document_id': 'source-long-id', 'page': 0, 'line': 2}}]
    original['directory_cases'] *= 2
    for case in original['directory_cases']:
        case['current']['body_windows'] = [
            {'document_id': 'source-long-id', 'page': 0, 'start_line': 0, 'end_line': 2}]
    ids = MaterialIds(original, contract=CURRENT)
    assert ids.payload['chapters']['blocks'][0]['rows'] == [['c1', ['d1', 0, 0]], ['c2', ['d1', 0, 2]]]
    assert ids.restore_payload() == original


def test_v3_bindings_include_contract_and_the_actual_permanent_mapping():
    original = payload()
    ids = MaterialIds(original, contract=CURRENT)
    assert ids.payload['identity_binding'] != MaterialIds(original).payload['identity_binding']
    changed = deepcopy(original)
    changed['documents'][0]['document_id'] = 'different-source'
    assert MaterialIds(changed, contract=CURRENT).payload['identity_binding'] != ids.payload['identity_binding']
    assert ids.payload == MaterialIds(deepcopy(original), contract=CURRENT).payload


@pytest.mark.parametrize('wrong', ['c999', 'd1', 'chapter-long-id'])
def test_v3_response_aliases_use_the_unchanged_short_id_response_schema(wrong):
    ids = MaterialIds(payload(), contract=CURRENT)
    assert ids.accept(lambda value: value)(response()) == MaterialIds(payload()).decode(response())
    raw = response()
    raw['assignments'][0]['chapter_id'] = wrong
    with pytest.raises(ModelContractError) as caught:
        ids.decode(raw)
    assert caught.value.errors[0]['path'] == ['assignments', 0, 'chapter_id']


@pytest.mark.parametrize('field,value,error_path', [
    ('basis_windows', [['d1', 0, 0, 2]], ['directory_adjudications', 0, 'basis_windows', 0]),
    ('assignments', {'blocks': []}, ['assignments']),
])
def test_v3_does_not_allow_input_record_or_coordinate_arrays_in_responses(field, value, error_path):
    raw = response()
    if field == 'basis_windows':
        raw['directory_adjudications'][0][field] = value
    else:
        raw[field] = value
    with pytest.raises(ModelContractError) as caught:
        MaterialIds(payload(), contract=CURRENT).accept(lambda decoded: _parse(MaterialPlan, decoded))(raw)
    assert caught.value.errors[0]['path'] == error_path


def test_unknown_contract_is_a_named_error():
    with pytest.raises(ModelContractError) as caught:
        MaterialIds(payload(), contract='material_short_ids_future')
    assert caught.value.errors[0]['code'] == 'directory_material_identity_contract_invalid'
