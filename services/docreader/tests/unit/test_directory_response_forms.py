"""Response-form selection is explicit, versioned and a separate bounded task."""

from copy import deepcopy
from io import BytesIO
import json
from zipfile import ZipFile

from docx import Document
import pytest

from docreader.llm.contracts import response_model
from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelBudgetError, ModelContractError
from docreader.tender import authoring_pipeline as pipeline
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.directory_compile import MaterialPlan, NativeMaterialPlan, compile_directory
from docreader.tender.directory_compile import outline_from_compiled
from docreader.tender.directory_response_forms import (
    RESPONSE_FORM_CONTRACT, ResponseFormView, bind_response_form_targets, response_form_catalog,
    require_response_form_budget, pending_response_form,
)
from docreader.tender.evidence.references import SourceIndex
from test_directory_compile import docs, fixture_titles, item, node, plan, window
from test_native_response_table import case


def test_native_material_operation_alone_advertises_response_form_selection():
    assert response_model('directory_material', {'operation': 'select_response_forms_v1'}) is NativeMaterialPlan
    assert response_model('directory_material', {}) is MaterialPlan
    assert 'response_forms' not in MaterialPlan.model_fields


def test_response_form_has_its_own_generation_contract():
    model = response_model('directory_response_form', {'form': {'grid': {'row_count': 7, 'column_count': 4}},
                                                     'atom_count': 10})
    assert {'layout', 'contexts', 'rows', 'source_guidance', 'dispositions'} <= model.model_fields.keys()


@pytest.mark.parametrize('location', ['template_parts', 'template_segments', 'material_segments'])
def test_compiled_part_locations_are_private_to_the_directory(tmp_path, location):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled, selection, target = source_case(artifacts)
    original = target['template_refs'][0]
    source_part = {**original, '_document_id': 'source', '_source_page': 0}
    chapter = compiled['chapters'][0]
    for field in ('template_parts', 'template_segments', 'material_segments'):
        chapter.pop(field, None)
    chapter[location] = [source_part] if location == 'template_parts' else [{'part': source_part}]
    before = deepcopy(compiled)

    catalog = response_form_catalog(compiled, documents)
    assert [entry['table_id'] for entry in catalog] == [selection['table_id']]
    view = ResponseFormView(artifacts, compiled, documents, selection, fixed)
    assert view.part == original
    assert view.accept(synthetic_response(view.payload))['complete']
    assert compiled == before


def source_case(artifacts):
    target, _, index = case(artifacts, count=3)
    native = deepcopy(index.raw)
    for unit in native['units']:
        unit['physical_locator'] = {'page_ordinal': unit['ordinal']}
    index = SourceIndex(index.document, native)
    index_ref = artifacts.put_json(index.descriptor(artifacts.put_json(native)))
    for part in target['template_refs']:
        part['source_ref']['source_index_ref'] = index_ref
    part = target['template_refs'][0]
    descriptor = artifacts.read_json(index_ref)
    fixed = {'schema_version': 2, 'run_id': 'response-scope', 'source_version': 1, 'domain_revision': 1,
             'frozen_ref': artifacts.put_json({'documents': [{**index.document, 'role': 'main',
                  'source_index_ref': index_ref, 'raw_ref': descriptor['raw_ref']}]})}
    pages = ['逐条评审响应表\n投标人应逐条列出以下评审要求原文。',
             '评审来源首格。', '评审来源续格。']
    documents = docs(pages)
    documents[0]['document_id'] = 'source'
    documents[0]['template_parts'] = {'form': part}
    documents[0]['grids'] = [{'unit_id': 'form', 'page': 0, 'grid': index.raw['units'][0]['grid']}]
    chosen = {**node('source-form', 0, 0, document_id='source'), 'body_windows': [window(0, 0, 1, 'source')]}
    compiled = compile_directory(plan([chosen], [item('source-form')]), documents)
    compiled['chapters'][0]['template_parts'] = [part]
    catalog = response_form_catalog(compiled, documents)
    assert len(catalog) == 1
    selection = {'table_id': catalog[0]['table_id'], 'mandate_windows': [window(0, 1, 1, 'source')],
                 'source_windows': [window(1, 0, 0, 'source'), window(2, 0, 0, 'source')]}
    return fixed, documents, compiled, selection, target


def synthetic_response(payload):
    """A labelled stub answer for the supplied synthetic native scope, never a real model receipt."""
    atoms = {ref: text for block in payload['source_scope'] for cell in block.get('cells', [])
             for ref, text in cell['lines']}
    first = next(ref for ref, text in atoms.items() if text.startswith('支持x86'))
    second = next(ref for ref, text in atoms.items() if text.startswith('芯）'))
    score = next(ref for ref, text in atoms.items() if text == '5.00')
    others = [ref for ref in atoms if ref not in {first, second, score}]
    return {'layout': {'repeat_start_row': 1, 'repeat_end_row': 5, 'sequence_column': 0,
                       'source_column': 1, 'response_column': 2, 'proof_column': 3},
            'contexts': [{'refs': [score]}], 'rows': [{'refs': [first, second], 'context_index': 0,
                                                    'source_join': 'continuation'}],
            'source_guidance': [], 'dispositions': [
                {'refs': [first, second], 'kind': 'criterion'}, {'refs': [score], 'kind': 'context'},
                {'refs': others, 'kind': 'navigation'}]}


def test_source_view_keeps_native_columns_and_complete_source_atoms_then_binds_after_ids(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled, selection, target = source_case(artifacts)
    view = ResponseFormView(artifacts, compiled, documents, selection, fixed)
    raw = synthetic_response(view.payload)
    compiled['_response_form_plans'] = [view.accept(raw)]
    target['target_id'] = 'chapter-7:target-2'
    target.pop('response_table')
    bind_response_form_targets(compiled, {'source-form': {'chapter_id': 'chapter-7', 'targets': [target]}})
    table = target['response_table']
    assert table['rows'][0]['row_id'].startswith('chapter-7:target-2:row-')
    assert table['rows'][0]['context_source_excerpts']
    doc, result = render_target(artifacts, target)
    assert len(doc.tables) == 1 and '海光、兆芯' in doc.tables[0].cell(1, 1).text
    assert not any(gap['code'] == 'export_native_response_table_unconfirmed' for gap in result['gaps'])


def render_target(artifacts, target):
    result = export_authoring_document(artifacts, {'chapters': [
        {'chapter_id': 'chapter-7', 'title': '响应原式', 'targets': [target], 'children': []}]}, [])
    return Document(BytesIO(artifacts.read_bytes(result['docx_ref']))), result


def test_missing_form_atom_stays_unresolved_without_another_round(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled, selection, _ = source_case(artifacts)
    view = ResponseFormView(artifacts, compiled, documents, selection, fixed)
    raw = synthetic_response(view.payload)
    raw['dispositions'][-1]['refs'].pop()
    accepted = view.accept(raw)
    assert not accepted['complete']
    assert any(entry['kind'] == 'unresolved' for entry in accepted['dispositions'])


@pytest.mark.parametrize('problem', ['duplicate_body', 'context_as_body', 'unknown_ref'])
def test_source_plan_cannot_hide_atoms_or_duplicate_clause_ownership(tmp_path, problem):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled, selection, _ = source_case(artifacts)
    view = ResponseFormView(artifacts, compiled, documents, selection, fixed)
    raw = synthetic_response(view.payload)
    if problem == 'duplicate_body':
        raw['rows'].append(deepcopy(raw['rows'][0]))
    elif problem == 'context_as_body':
        raw['rows'][0]['refs'].append(raw['contexts'][0]['refs'][0])
    else:
        raw['rows'][0]['refs'] = [view.payload['atom_count'] + 1]
    with pytest.raises(ModelContractError):
        view.accept(raw)


def test_unresolved_scope_preserves_original_form_instead_of_publishing_partial_rows(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled, selection, target = source_case(artifacts)
    view = ResponseFormView(artifacts, compiled, documents, selection, fixed)
    raw = synthetic_response(view.payload)
    unresolved = raw['dispositions'][-1]['refs'].pop()
    raw['dispositions'].append({'refs': [unresolved], 'kind': 'unresolved'})
    accepted = view.accept(raw)
    assert not accepted['complete']
    compiled['_response_form_plans'] = [accepted]
    target.pop('response_table')
    bind_response_form_targets(compiled, {'source-form': {'targets': [target]}})
    assert 'response_table' not in target


async def test_pipeline_only_new_contract_adds_native_material_scope_and_one_distinct_F_call(tmp_path, monkeypatch):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled, selection, _ = source_case(artifacts)
    calls = []

    async def ask(_execute, _budget, task, payload, validate):
        calls.append((str(task), deepcopy(payload)))
        if task == 'directory_material':
            assert payload['operation'] == 'select_response_forms_v1'
            assert 'grid' not in payload['response_form_catalog'][0]
            return validate({'assignments': [], 'chapter_titles': fixture_titles(payload), 'response_forms': [selection]})
        if task == 'directory_requirement':
            return validate({'assignments': []})
        if task == 'directory_classification':
            return validate({'items': []})
        assert task == 'directory_response_form'
        return validate(synthetic_response(payload))

    monkeypatch.setattr(pipeline, '_ask', ask)
    await pipeline._request_directory_details(compiled, documents, None, None, fixed, artifacts=artifacts,
                                               response_form_contract=RESPONSE_FORM_CONTRACT)
    assert [task for task, _ in calls].count('directory_response_form') == 1
    assert compiled['_response_form_plans'][0]['complete']


async def test_synthetic_pipeline_stub_reaches_same_native_word_and_zip_with_blank_answers(tmp_path, monkeypatch):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled, selection, _ = source_case(artifacts)
    calls = []

    async def ask(_execute, _budget, task, payload, validate):
        calls.append(str(task))
        if task == 'directory_material':
            return validate({'chapter_titles': fixture_titles(payload), 'response_forms': [selection]})
        if task == 'directory_requirement':
            return validate({'assignments': []})
        if task == 'directory_classification':
            return validate({'items': []})
        assert task == 'directory_response_form'
        return validate(synthetic_response(payload))

    with monkeypatch.context() as context:
        context.setattr(pipeline, '_ask', ask)
        await pipeline._request_directory_details(compiled, documents, None, None, fixed, artifacts=artifacts,
                                                   response_form_contract=RESPONSE_FORM_CONTRACT)
    outline = outline_from_compiled(fixed, compiled)
    assert any(target.get('response_table') for chapter in outline['chapters'] for target in chapter['targets'])

    async def model(task, request, validate):
        assert task == 'authoring_structure'
        return validate({'findings': []})

    budget = RequestBudget(model='fixture', context_tokens=131072, tokenizer='o200k_base', tokenized_authoring=True)
    result = artifacts.read_json(await pipeline.generate_document(artifacts, fixed, budget, model, outline=outline))
    word = artifacts.read_bytes(result['docx_ref'])
    doc = Document(BytesIO(word))
    assert len(doc.tables) == 1 and len(doc.tables[0].columns) == 4
    assert '海光、兆芯' in doc.tables[0].cell(1, 1).text
    assert all(not row.cells[2].text and not row.cells[3].text for row in doc.tables[0].rows[1:])
    with ZipFile(BytesIO(artifacts.read_bytes(result['package_ref']))) as package:
        manifest = json.loads(package.read('manifest.json'))
        assert package.read(manifest['document']['package_path']) == word
    assert calls.count('directory_response_form') == 1


async def test_F_output_budget_failure_happens_before_any_execute_call(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled, selection, _ = source_case(artifacts)
    view = ResponseFormView(artifacts, compiled, documents, selection, fixed)
    view.payload['atom_count'] = 100000

    async def execute(*_):
        pytest.fail('An unadmitted response form must not be sent.')

    budget = RequestBudget(model='fixture', context_tokens=131072, tokenizer='o200k_base', tokenized_authoring=True)
    with pytest.raises(ModelBudgetError):
        await pipeline._ask(execute, budget, 'directory_response_form', view.payload, lambda raw: raw)


def test_complete_request_budget_rejects_before_execute_when_output_shape_cannot_fit(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled, selection, _ = source_case(artifacts)
    view = ResponseFormView(artifacts, compiled, documents, selection, fixed)
    budget = RequestBudget(model='fixture', context_tokens=131072, tokenizer='o200k_base', tokenized_authoring=True)
    request = pipeline._request(budget, 'directory_response_form', view.payload)
    measured = require_response_form_budget(budget, request, view.payload)
    assert measured['input_tokens'] > 0 and measured['worst_accepted_output_tokens'] <= measured['output_reserve']
    huge = deepcopy(view.payload)
    huge['atom_count'] = 100000
    with pytest.raises(ModelBudgetError):
        require_response_form_budget(budget, request, huge)


def test_pdf_embedded_file_inventory_is_not_mistaken_for_unlocated_page_text(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled, selection, _ = source_case(artifacts)
    frozen = artifacts.read_json(fixed['frozen_ref'])
    record = frozen['documents'][0]
    raw = artifacts.read_json(record['raw_ref'])
    raw['units'].append({'unit_id': 'attachment:0', 'kind': 'embedded_file', 'ordinal': len(raw['units']),
                         'text': 'source-original.docx', 'locator': {'locator_kind': 'attachment'}})
    index = SourceIndex({key: record[key] for key in ('document_id', 'document_version', 'original_ref')}, raw)
    record.update(raw_ref=artifacts.put_json(raw), source_index_ref=artifacts.put_json(index.descriptor(artifacts.put_json(raw))))
    fixed['frozen_ref'] = artifacts.put_json(frozen)
    view = ResponseFormView(artifacts, compiled, documents, selection, fixed)
    assert all('source-original.docx' not in text for block in view.payload['source_scope']
               for cell in block.get('cells', []) for _, text in cell['lines'])


async def test_new_material_wire_preserves_short_ids_coordinates_and_base_prompt(tmp_path):
    from docreader.tender.directory_material_ids import MaterialIds

    artifacts = ArtifactStore(tmp_path)
    _, documents, compiled, selection, _ = source_case(artifacts)
    catalog = response_form_catalog(compiled, documents)
    payload = {'operation': 'select_response_forms_v1', 'response_form_catalog': catalog,
               'documents': [{'document_id': 'source', 'pages': [{'page': 0, 'lines': [[0, 'source text']]}]}]}
    projected = MaterialIds(payload)
    assert projected.restore_payload() == payload
    seen = []
    configured = RequestBudget(model='fixture', prompts={'directory_material': 'BASE MATERIAL PROMPT',
        'directory_material_response_forms': 'FORM SCOPE DELTA'})

    async def execute(task, request, validate):
        sent = json.loads(request['messages'][1]['content'][0]['text'])
        assert request['messages'][0]['content'].startswith('BASE MATERIAL PROMPT\n\nFORM SCOPE DELTA')
        assert json.loads(request['messages'][0]['content'].splitlines()[-1])['title'] == 'NativeMaterialPlan'
        assert sent['response_form_catalog'][0]['table_id'] == 't1'
        assert sent['response_form_catalog'][0]['document_id'] == 'd1'
        return validate({'response_forms': [{**selection, 'table_id': 't1',
            'mandate_windows': [{**selection['mandate_windows'][0], 'document_id': 'd1'}],
            'source_windows': [{**window, 'document_id': 'd1'} for window in selection['source_windows']]}]})

    def accept(raw):
        value = NativeMaterialPlan.model_validate(raw).model_dump()
        seen.append(value)
        assert value['response_forms'] == [selection]
        return value

    await pipeline._ask(execute, configured, 'directory_material', payload, accept)
    assert len(seen) == 1
    old = pipeline._request(configured, 'directory_material', {'documents': payload['documents']})
    assert old['messages'][0]['content'].startswith('BASE MATERIAL PROMPT\n\n')
    assert 'FORM SCOPE DELTA' not in old['messages'][0]['content']
    assert json.loads(old['messages'][0]['content'].splitlines()[-1])['title'] == 'MaterialPlan'


def test_failed_form_has_bound_pending_ranges_and_guide_without_publishing_full_source_as_answers(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled, selection, target = source_case(artifacts)
    compiled['_response_form_pending'] = [pending_response_form(artifacts, compiled, documents, selection, fixed)]
    target.pop('response_table')
    bind_response_form_targets(compiled, {'source-form': {'targets': [target]}})
    pending = target['response_form_pending']
    assert pending['table_source_ref'] == target['template_refs'][0]['source_ref']
    assert pending['source_windows'][0]['source_index_ref']
    doc, result = render_target(artifacts, target)
    assert len(doc.tables) == 1 and len(doc.tables[0].rows) == 7
    assert '逐条条款尚未完整展开' in '\n'.join(p.text for p in doc.paragraphs)
    with ZipFile(BytesIO(artifacts.read_bytes(result['package_ref']))) as package:
        notes = package.read('提交要求与待办.md').decode()
    assert '完整条款来源范围：《已登记来源1》物理第2页' in notes
    assert '原式展开依据：《已登记来源1》物理第1页' in notes
    assert not any(row.cells[1].text for row in doc.tables[0].rows[1:])


def test_reference_and_procedure_are_guidance_only_not_enterprise_answer_cells(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled, selection, target = source_case(artifacts)
    view = ResponseFormView(artifacts, compiled, documents, selection, fixed)
    raw = synthetic_response(view.payload)
    reference = raw['dispositions'][-1]['refs'].pop()
    raw['dispositions'].append({'refs': [reference], 'kind': 'reference'})
    raw['source_guidance'] = [{'kind': 'reference', 'refs': [reference]}]
    compiled['_response_form_plans'] = [view.accept(raw)]
    target.pop('response_table')
    bind_response_form_targets(compiled, {'source-form': {'targets': [target]}})
    doc, result = render_target(artifacts, target)
    assert not any(row.cells[2].text or row.cells[3].text for row in doc.tables[0].rows[1:])
    with ZipFile(BytesIO(artifacts.read_bytes(result['package_ref']))) as package:
        notes = package.read('提交要求与待办.md').decode()
    assert '编制参考：响应原式' in notes and '不表示投标人已履行或已经满足' in notes


def test_source_body_refs_cannot_reverse_their_original_cell_or_page_order(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled, selection, _ = source_case(artifacts)
    view = ResponseFormView(artifacts, compiled, documents, selection, fixed)
    raw = synthetic_response(view.payload)
    raw['rows'][0]['refs'].reverse()
    raw['rows'][0]['source_join'] = 'paragraphs'
    with pytest.raises(ModelContractError):
        view.accept(raw)


@pytest.mark.parametrize('problem', ['reverse', 'another_frozen_document'])
def test_source_guidance_continuation_is_checked_before_a_complete_receipt(tmp_path, problem):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled, selection, _ = source_case(artifacts)
    if problem == 'another_frozen_document':
        frozen = artifacts.read_json(fixed['frozen_ref'])
        record = frozen['documents'][0]
        raw = artifacts.read_json(record['raw_ref'])
        other = SourceIndex({'document_id': 'other', 'document_version': '1', 'original_ref': record['original_ref']}, raw)
        frozen['documents'].append({**other.document, 'role': 'attachment', 'raw_ref': record['raw_ref'],
            'source_index_ref': artifacts.put_json(other.descriptor(record['raw_ref']))})
        fixed['frozen_ref'] = artifacts.put_json(frozen)
        documents.append({'document_id': 'other', 'role': 'attachment', 'pages': list(documents[0]['pages'])})
        selection['source_windows'].append(window(2, 0, 0, 'other'))
    view = ResponseFormView(artifacts, compiled, documents, selection, fixed)
    raw = synthetic_response(view.payload)
    navigation = raw['dispositions'][-1]['refs']
    refs = [navigation.pop(0), navigation.pop(-1)]
    if problem == 'reverse':
        refs.reverse()
    raw['dispositions'].append({'refs': sorted(refs), 'kind': 'procedure'})
    raw['source_guidance'] = [{'refs': refs, 'kind': 'procedure', 'source_join': 'continuation'}]
    with pytest.raises(ModelContractError):
        view.accept(raw)


def test_host_orders_complete_clause_rows_by_first_owned_source_atom(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled, selection, _ = source_case(artifacts)
    view = ResponseFormView(artifacts, compiled, documents, selection, fixed)
    raw = synthetic_response(view.payload)
    first, second = raw['rows'][0]['refs']
    raw['rows'] = [{'refs': [second], 'context_index': 0}, {'refs': [first], 'context_index': 0}]
    accepted = view.accept(raw)
    rows = accepted['response_table']['rows']
    assert rows[0]['source_excerpts'] == view.excerpts([first])
    assert rows[1]['source_excerpts'] == view.excerpts([second])
