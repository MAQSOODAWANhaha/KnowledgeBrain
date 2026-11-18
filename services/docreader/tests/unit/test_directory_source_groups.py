"""A material decision can restore source-owned groups without rebuilding chapters."""

from copy import deepcopy
from io import BytesIO
import json
from zipfile import ZipFile

from docx import Document
import pytest
from pydantic import ValidationError

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.contracts import output_schema, response_model
from docreader.llm.request import ModelContractError
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.directory_compile import (
    MaterialPlan, NativeMaterialPlan, compile_directory, outline_from_compiled,
)
from docreader.tender.directory_material_ids import MaterialIds
from test_directory_compile import docs, fixture_titles, item, node, plan, point, window


def grouping():
    from docreader.tender import directory_source_groups
    return directory_source_groups


def source_case():
    documents = docs(['申请文件由下列两册组成\n资信册\n方案册',
                      '资信表\n固定原式与签章', '服务方案\n完整技术范围', '人员表\n姓名：________'])
    raw = plan([
        {**node('form', 1, 0), 'body_windows': [window(1, 0, 1)]},
        {**node('solution', 2, 0), 'response_kind': 'narrative', 'body_windows': [window(2, 0, 1)]},
        {**node('team', 3, 0, parent='solution'), 'body_windows': [window(3, 0, 1)]},
    ], [item('form'), item('solution'), item('team')])
    return documents, compile_directory(raw, documents)


def selections():
    return [
        {'heading': point(0, 1), 'basis_windows': [window(0, 0, 1)], 'chapter_ids': ['form'], 'title': '资信文件'},
        {'heading': point(0, 2), 'basis_windows': [window(0, 0, 2)], 'chapter_ids': ['solution'], 'title': '服务方案'},
    ]


def snapshot():
    return {'schema_version': 2, 'run_id': 'restore-source-groups', 'source_version': 1, 'domain_revision': 1}


def test_new_material_contract_is_additive_and_old_schemas_reject_the_new_field():
    raw = {'source_groups': selections()}
    for model in (MaterialPlan, NativeMaterialPlan):
        with pytest.raises(ValidationError):
            model.model_validate(raw)
    model = grouping().MaterialSourceGroupPlan
    assert model.model_validate(raw).model_dump()['source_groups'] == raw['source_groups']
    assert response_model('directory_material', {'operation': grouping().SOURCE_GROUP_OPERATION}) is model
    assert 'source_groups' not in output_schema('directory_material', {})['properties']
    assert 'source_groups' not in output_schema('directory_material', {'operation': 'select_response_forms_v1'})['properties']


def test_groups_follow_source_order_and_keep_every_existing_body_subject_and_subtree(tmp_path):
    documents, compiled = source_case()
    compiled['chapters'] = [compiled['chapters'][1], compiled['chapters'][2], compiled['chapters'][0]]
    compiled['chapters'][-1]['subjects'] = [{'role': '申请人'}, {'role': '制造商'}]
    before = deepcopy(compiled['chapters'])
    grouping().apply_source_groups(compiled, documents, list(reversed(selections())))
    for actual, original in zip(compiled['chapters'], before):
        for field in original:
            if field != 'group':
                assert actual[field] == original[field]
    outline = outline_from_compiled(snapshot(), compiled)
    assert [chapter['title'] for chapter in outline['chapters']] == ['资信文件', '服务方案']
    assert all(chapter['is_volume'] for chapter in outline['chapters'])
    first, second = outline['chapters']
    assert {target['subject'] for target in first['children'][0]['targets']} == {'申请人', '制造商'}
    assert second['children'][0]['children'][0]['title'] == '人员表'
    assert len(compiled['_source_groups']) == 2
    assert compiled['_source_groups'][0]['basis_windows'] == selections()[0]['basis_windows']
    basis = [issue for issue in outline['issues'] if issue['code'] == 'outline_directory_source_decision']
    assert any('申请文件由下列两册组成\n资信册' in issue['detail'] for issue in basis)
    artifacts = ArtifactStore(tmp_path / 'artifacts')
    exported = export_authoring_document(artifacts, outline, [])
    with ZipFile(BytesIO(artifacts.read_bytes(exported['package_ref']))) as package:
        manifest = json.loads(package.read('manifest.json'))
        assert [volume['title'] for volume in manifest['volumes']] == ['资信文件', '服务方案']
        text = ['\n'.join(p.text for p in Document(BytesIO(package.read(v['package_path']))).paragraphs)
                for v in manifest['volumes']]
        assert '固定原式与签章' in text[0] and '完整技术范围' not in text[0]
        assert '完整技术范围' in text[1] and '姓名：________' in text[1]


@pytest.mark.parametrize('failure', ['repeated_member', 'unknown_member', 'descendant', 'cover', 'content', 'grouped',
                                    'missing_heading_basis', 'empty_basis', 'bad_heading', 'bad_title', 'duplicate_group'])
def test_invalid_group_batch_is_rejected_before_any_chapter_changes(failure):
    documents, compiled = source_case()
    selected = selections()
    if failure == 'repeated_member':
        selected[1]['chapter_ids'].append('form')
    elif failure == 'unknown_member':
        selected[1]['chapter_ids'] = ['unknown']
    elif failure == 'descendant':
        selected[1]['chapter_ids'] = ['team']
    elif failure in ('cover', 'content'):
        compiled['chapters'][1]['presentation'] = failure
    elif failure == 'grouped':
        compiled['chapters'][1].update(group='旧原册', group_heading=point(0, 0))
    elif failure == 'missing_heading_basis':
        selected[1]['basis_windows'] = [window(0, 0)]
    elif failure == 'empty_basis':
        selected[1]['basis_windows'] = []
    elif failure == 'bad_heading':
        selected[1]['heading'] = point(99, 0)
    elif failure == 'bad_title':
        selected[1]['title'] = '方案册应包括：'
    elif failure == 'duplicate_group':
        selected[1]['heading'] = selected[0]['heading']
    before = deepcopy(compiled)
    with pytest.raises((ModelContractError, ValidationError)):
        grouping().apply_source_groups(compiled, documents, selected)
    assert compiled == before


def test_group_cannot_move_a_root_from_an_unrelated_document():
    documents, compiled = source_case()
    documents.append({'document_id': 'other', 'role': 'attachment', 'pages': ['另一组\n补充范围']})
    selected = selections()[:1]
    selected[0].update(heading=point(0, 0, document_id='other'),
                       basis_windows=[window(0, 0, 1, document_id='other')])
    before = deepcopy(compiled)
    with pytest.raises(ModelContractError):
        grouping().apply_source_groups(compiled, documents, selected)
    assert compiled == before


def test_material_aliases_restore_group_source_coordinates_and_existing_chapters():
    documents, compiled = source_case()
    payload = {'operation': grouping().SOURCE_GROUP_OPERATION,
               'documents': [{'document_id': d['document_id']} for d in documents],
               'chapters': [{'id': c['id']} for c in compiled['chapters']]}
    identities = MaterialIds(payload)
    raw = deepcopy(selections())
    for value in raw:
        value['heading']['document_id'] = identities.forward['d']['tender']
        for basis in value['basis_windows']:
            basis['document_id'] = identities.forward['d']['tender']
        value['chapter_ids'] = [identities.forward['c'][identity] for identity in value['chapter_ids']]
    assert identities.decode({'source_groups': raw}) == {'source_groups': selections()}


def test_valid_numbered_source_anchor_with_complete_basis_does_not_silently_drop_its_group():
    documents, compiled = source_case()
    documents[0]['pages'][0] = '申请文件由下列两册组成\n1.\n方案册'
    grouping().apply_source_groups(compiled, documents, selections()[:1])
    outline = outline_from_compiled(snapshot(), compiled)
    assert outline['chapters'][0]['title'] == '资信文件' and outline['chapters'][0]['is_volume']


def test_new_material_group_validation_reports_its_field_and_preserves_original_state():
    from docreader.tender.directory_titles import title_catalog
    documents, compiled = source_case()
    selected = selections()
    selected[1]['chapter_ids'] = ['team']
    raw = {'chapter_titles': [{'title_id': entry['title_id'], 'title': entry['adopted_source_name']}
                             for entry in title_catalog(compiled, documents)], 'source_groups': selected}
    original = deepcopy(compiled)
    with pytest.raises(ModelContractError) as caught:
        pipeline._accept_material_plan(raw, [], compiled, documents,
                                       source_group_contract=grouping().SOURCE_GROUP_CONTRACT)
    assert caught.value.errors[0]['path'] == ['source_groups', 1, 'chapter_ids', 0]
    assert compiled == original


async def test_new_group_contract_uses_one_material_request_and_applies_groups_atomically(monkeypatch):
    documents, compiled = source_case()
    calls = []

    async def ask(_execute, _budget, task, payload, validate):
        calls.append((str(task), deepcopy(payload)))
        if task == 'directory_material':
            assert payload['operation'] == grouping().SOURCE_GROUP_OPERATION
            assert {value['chapter_id'] for value in payload['source_group_candidates']} == {'form', 'solution', 'team'}
            return validate({'chapter_titles': fixture_titles(payload), 'source_groups': selections()})
        return validate({'assignments': []} if task == 'directory_requirement' else {'items': []})

    monkeypatch.setattr(pipeline, '_ask', ask)
    await pipeline._request_directory_details(compiled, documents, None, None, snapshot(),
                                             source_group_contract=grouping().SOURCE_GROUP_CONTRACT)
    assert [task for task, _ in calls].count('directory_material') == 1
    assert len(outline_from_compiled(snapshot(), compiled)['chapters']) == 2
    assert all(chapter['is_volume'] for chapter in outline_from_compiled(snapshot(), compiled)['chapters'])


async def test_generate_propagates_the_explicit_source_group_contract(tmp_path, monkeypatch):
    documents, compiled = source_case()
    observed = []

    async def plans(*_args, **_options):
        return [], []

    async def details(value, _documents, _budget, _execute, _identity, **options):
        observed.append(options['source_group_contract'])
        grouping().apply_source_groups(value, documents, selections())
        return value

    async def no_model(*_args):
        raise AssertionError('this controlled compilation must not send a model request')

    monkeypatch.setattr(pipeline, '_request_directory_plans', plans)
    monkeypatch.setattr(pipeline, 'compile_plans', lambda *_args: deepcopy(compiled))
    monkeypatch.setattr(pipeline, '_request_directory_details', details)
    artifacts = ArtifactStore(tmp_path / 'artifacts')
    result = artifacts.read_json(await pipeline.generate_document(
        artifacts, snapshot(), RequestBudget(model='synthetic'), no_model, outline_only=True,
        directory_documents=documents, source_group_contract=grouping().SOURCE_GROUP_CONTRACT))
    assert observed == [grouping().SOURCE_GROUP_CONTRACT]
    assert len(result['chapters']) == 2 and all(chapter['is_volume'] for chapter in result['chapters'])


def test_source_group_prompt_and_output_contract_only_activate_for_the_new_operation():
    module = grouping()
    budget = RequestBudget(model='synthetic', context_tokens=131072)
    legacy = pipeline._request(budget, 'directory_material', {'operation': 'select_response_forms_v1'})
    new = pipeline._request(budget, 'directory_material', {'operation': module.SOURCE_GROUP_OPERATION})
    assert 'source_groups' not in legacy['messages'][0]['content']
    assert 'source_groups' in new['messages'][0]['content']
    assert '来源分册' in new['messages'][0]['content']
    documents, compiled = source_case()
    with pytest.raises(ModelContractError):
        pipeline._accept_material_plan({'source_groups': selections()}, [], compiled, documents)
