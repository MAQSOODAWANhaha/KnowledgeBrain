"""Reading navigation has source provenance but never settles a material node."""

from copy import deepcopy

import pytest
from pydantic import ValidationError

from docreader.llm.request import ModelContractError
from docreader.tender.directory_compile import DirectoryPlan, DirectoryPlanRejected, compile_directory, compile_plans
from docreader.tender.directory_ref_plan import RefDirectoryPlan, decode_plan
from docreader.tender.directory_sources import SourceRefs


DOCUMENTS = [{'document_id': 'source', 'role': 'main', 'pages': [
    '商务文件\n投标函\n固定声明\n签字：________\n\n技术文件包括技术实施计划\n技术实施计划\n参考样表\n本项目不要求提供参考样表\n格式目录\n代理商投标时提供']}]


def response():
    return {'nodes': [{'heading_ref': 1, 'body_ranges': [[2, 3]], 'response_kind': 'form'}],
            'composition': {'entries': [{'name_ref': 1}]},
            'scopes': [{'items': [{'node_ref': 1, 'entry_ref': 1, 'depth': 'self', 'presentation': 'section'}]}]}


def v2(raw, **options):
    from docreader.tender.directory_ref_plan import RefDirectoryPlanV2
    return decode_plan(raw, SourceRefs(DOCUMENTS), model=RefDirectoryPlanV2, **options)


def test_explicit_v2_keeps_reading_navigation_outside_nodes_and_bid_body():
    from docreader.tender.directory_compile import ReadingDirectoryPlan

    raw = {**response(), 'reading_only_refs': [0, 9]}
    before = deepcopy(raw)
    decoded = v2(raw)
    assert len(decoded['nodes']) == 1
    origins = [{'document_id': 'source', 'page': 0, 'line': line} for line in (0, 9)]
    assert decoded['reading_only_sources'] == origins
    ReadingDirectoryPlan.model_validate(decoded)
    compiled = compile_directory(decoded, DOCUMENTS)
    assert compiled['_reading_references'] == origins
    assert compiled['chapters'][0]['text'] == '固定声明\n签字：________'
    assert len(compiled['chapters']) == 1 and compiled['issues'] == []
    assert raw == before


def test_v1_schema_and_explicit_dispatch_never_infer_v2_from_an_extra_field():
    from docreader.tender.directory_ref_plan import (
        DIRECTORY_REF_CONTRACT, LEGACY_DIRECTORY_REF_CONTRACT, RefDirectoryPlanV2,
    )

    assert DIRECTORY_REF_CONTRACT == 'source_refs_compact_plan_v12'
    assert LEGACY_DIRECTORY_REF_CONTRACT == 'source_refs_compact_plan_v11'
    assert 'reading_only_refs' not in RefDirectoryPlan.model_json_schema()['properties']
    assert RefDirectoryPlanV2.model_json_schema()['properties']['reading_only_refs']['items']['type'] == 'integer'
    raw = {**response(), 'reading_only_refs': [0]}
    with pytest.raises(ModelContractError) as caught:
        decode_plan(raw, SourceRefs(DOCUMENTS))
    assert caught.value.errors[0]['path'] == ['reading_only_refs']
    assert 'reading_only_sources' not in decode_plan(response(), SourceRefs(DOCUMENTS))
    with pytest.raises(ValidationError):
        DirectoryPlan.model_validate(v2(raw))


@pytest.mark.parametrize('new_contract', [False, True])
def test_actual_failure_shape_with_undeclared_source_only_stays_rejected(new_contract):
    # Faithful minimal shape of v33: declared form plus non-node navigation refs.
    raw = {**response(), 'source_only': [0, 9]}
    before = deepcopy(raw)
    with pytest.raises(ModelContractError) as caught:
        (v2(raw) if new_contract else decode_plan(raw, SourceRefs(DOCUMENTS)))
    assert [(error['path'], error['ref']) for error in caught.value.errors] == [
        (['source_only', 0], 0), (['source_only', 1], 9)]
    assert all('declared node' in error['detail'] for error in caught.value.errors)
    assert all(('reading_only_refs' in error['detail']) == new_contract for error in caught.value.errors)
    assert raw == before


@pytest.mark.parametrize('new_contract', [False, True])
def test_all_unknown_node_selections_are_reported_together_with_paths_and_refs(new_contract):
    raw = response()
    raw['nodes'][0]['parent_ref'] = 0
    raw['scopes'][0]['items'][0]['node_ref'] = 2
    raw['excluded'] = [{'node_ref': 3, 'source': [8, 8]}]
    raw['source_only'] = [9]
    raw['format_bindings'] = [{'source': [8, 8], 'response_node_ref': 6, 'reference_node_refs': [7, 10]}]
    with pytest.raises(ModelContractError) as caught:
        (v2(raw) if new_contract else decode_plan(raw, SourceRefs(DOCUMENTS)))
    assert [(error['path'], error['ref']) for error in caught.value.errors] == [
        (['nodes', 0, 'parent_ref'], 0), (['scopes', 0, 'items', 0, 'node_ref'], 2),
        (['excluded', 0, 'node_ref'], 3), (['source_only', 0], 9),
        (['format_bindings', 0, 'response_node_ref'], 6),
        (['format_bindings', 0, 'reference_node_refs', 0], 7),
        (['format_bindings', 0, 'reference_node_refs', 1], 10)]
    assert all('固定声明' not in error['detail'] for error in caught.value.errors)


def test_supplied_candidates_do_not_let_node_selections_escape_the_declared_plan():
    raw = {**response(), 'source_only': [0]}
    with pytest.raises(ModelContractError) as caught:
        v2(raw, known_node_refs={0, 1})
    assert caught.value.errors[0]['path'] == ['source_only', 0]


@pytest.mark.parametrize('reference,options', [
    (99, {}), (-1, {}), (True, {}), (4, {}), (0, {'visible': {1, 2, 3}}), (1, {}),
])
def test_reading_ref_must_be_visible_known_nonempty_integer_and_not_a_declared_node(reference, options):
    with pytest.raises(ModelContractError) as caught:
        v2({**response(), 'reading_only_refs': [reference]}, **options)
    assert caught.value.errors[0]['path'] == ['reading_only_refs', 0]


def test_reading_ref_duplicates_are_rejected_at_the_duplicate_position():
    with pytest.raises(ModelContractError) as caught:
        v2({**response(), 'reading_only_refs': [0, 0]})
    assert caught.value.errors[0]['path'] == ['reading_only_refs', 1]
    assert caught.value.errors[0]['ref'] == 0


def test_declared_source_only_nodes_still_have_the_original_destination():
    raw = response()
    raw['nodes'].append({'heading_ref': 9, 'body_ranges': [], 'response_kind': 'narrative'})
    raw.update(source_only=[9], reading_only_refs=[0])
    decoded = v2(raw)
    assert decoded['source_only'] == [decoded['nodes'][1]['id']]
    assert len(compile_directory(decoded, DOCUMENTS)['chapters']) == 1


@pytest.mark.parametrize('problem', ['undecided_node', 'independent_entry', 'unknown_parent', 'unknown_body', 'unknown_entry'])
def test_reading_selection_cannot_bypass_existing_plan_closure(problem):
    raw = {**response(), 'reading_only_refs': [9]}
    if problem == 'undecided_node':
        raw['nodes'].append({'heading_ref': 6, 'body_ranges': [], 'response_kind': 'narrative'})
    elif problem == 'independent_entry':
        raw['composition']['entries'].append({'name_ref': 6})
    elif problem == 'unknown_parent':
        raw['nodes'][0]['parent_ref'] = 6
    elif problem == 'unknown_body':
        raw['nodes'][0]['body_ranges'] = [[2, 99]]
    else:
        raw['scopes'][0]['items'][0]['entry_ref'] = 6
    with pytest.raises((ModelContractError, DirectoryPlanRejected)):
        compile_directory(v2(raw), DOCUMENTS)


def test_invalid_entry_selection_identifies_its_path_and_reference():
    raw = response()
    raw['scopes'][0]['items'][0]['entry_ref'] = 6
    with pytest.raises(ModelContractError) as caught:
        v2(raw)
    assert caught.value.errors[0]['path'] == ['scopes', 0, 'items', 0, 'entry_ref']
    assert caught.value.errors[0]['ref'] == 6


def test_reading_an_independent_entry_does_not_settle_that_material():
    raw = {**response(), 'reading_only_refs': [6]}
    raw['composition']['entries'].append({'name_ref': 6})
    with pytest.raises(DirectoryPlanRejected, match='composition entries without a decision'):
        compile_directory(v2(raw), DOCUMENTS)


def test_invalid_body_range_keeps_the_source_field_path_and_endpoints():
    raw = {**response(), 'reading_only_refs': [9]}
    raw['nodes'][0]['body_ranges'] = [[2, 99]]
    with pytest.raises(ModelContractError) as caught:
        v2(raw)
    assert caught.value.errors[0]['path'] == ['nodes', 0, 'body_ranges', 0]
    assert caught.value.errors[0]['ref'] == [2, 99]


def test_empty_body_for_a_real_technical_plan_keeps_its_independent_material_and_basis():
    raw = response()
    raw['nodes'].append({'heading_ref': 6, 'body_ranges': [], 'response_kind': 'narrative'})
    raw['composition']['entries'].append({'name_ref': 6})
    raw['scopes'][0]['items'].append({'node_ref': 6, 'entry_ref': 6, 'depth': 'self',
                                     'presentation': 'section', 'clause': [[5, 5]], 'condition': [10, 10]})
    raw['reading_only_refs'] = [9]
    compiled = compile_directory(v2(raw), DOCUMENTS)
    technical = compiled['chapters'][1]
    assert technical['title'] == '技术实施计划' and technical['response_kind'] == 'narrative'
    assert technical['text'] == '' and technical['body_windows'] == []
    assert '技术文件包括技术实施计划' in technical['obligations']
    assert technical['condition'] == '代理商投标时提供'


def test_parent_child_body_ownership_still_rejects_duplicate_material_body():
    raw = {**response(), 'reading_only_refs': [9]}
    raw['nodes'].append({'heading_ref': 6, 'body_ranges': [[2, 3]], 'response_kind': 'form', 'parent_ref': 1})
    raw['scopes'][0]['items'][0]['depth'] = 'subtree'
    with pytest.raises(DirectoryPlanRejected, match='duplicate body ownership'):
        compile_directory(v2(raw), DOCUMENTS)


def test_reading_metadata_survives_two_plan_compilation_without_affecting_chapters():
    first = v2({**response(), 'reading_only_refs': [0]})
    second = v2({**response(), 'reading_only_refs': [9]})
    compiled = compile_plans([first, second], DOCUMENTS)
    assert compiled['_reading_references'] == [
        {'document_id': 'source', 'page': 0, 'line': line} for line in (0, 9)]
    assert compiled['chapters'] == compile_directory(first, DOCUMENTS)['chapters']


@pytest.mark.parametrize('change', ['unknown', 'empty', 'duplicate', 'declared', 'boolean'])
def test_internal_reading_sources_are_validated_again_by_the_compiler(change):
    decoded = v2({**response(), 'reading_only_refs': [0]})
    point = decoded['reading_only_sources'][0]
    if change == 'unknown':
        point['line'] = 99
    elif change == 'empty':
        point['line'] = 4
    elif change == 'duplicate':
        decoded['reading_only_sources'].append(deepcopy(point))
    elif change == 'declared':
        point['line'] = 1
    else:
        point['line'] = False
    with pytest.raises(DirectoryPlanRejected):
        compile_directory(decoded, DOCUMENTS)
