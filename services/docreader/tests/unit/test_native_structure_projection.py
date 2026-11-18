"""Actual native text/role changes must change full and compact structure requests."""

from copy import deepcopy
from io import BytesIO
import hashlib
import json
from zipfile import ZipFile

from docx import Document
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.llm.budget import RequestBudget
from docreader.llm.request import ModelBudgetError, build_request, request_digest
from docreader.tender import authoring_structure as structure
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.authoring_view import compact_structure_payload
from docreader.tender.directory_response_forms import ResponseFormView, bind_response_form_targets
from test_directory_response_forms import source_case, synthetic_response
from test_native_response_table import rebind
from test_structure_section_view import expanded_structure


def fixture(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled, selection, target = source_case(artifacts)
    view = ResponseFormView(artifacts, compiled, documents, selection, fixed)
    compiled['_response_form_plans'] = [view.accept(synthetic_response(view.payload))]
    target.pop('response_table')
    bind_response_form_targets(compiled, {'source-form': {'targets': [target]}})
    outline = {'authoring_input': fixed, 'chapters': [{'chapter_id': 'chapter', 'title': '评审响应',
               'targets': [target], 'children': []}], 'issues': []}
    return artifacts, outline, view


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':'))


def actual_text(artifacts, outline, guide=False):
    result = export_authoring_document(artifacts, outline, [])
    if guide:
        with ZipFile(BytesIO(artifacts.read_bytes(result['package_ref']))) as package:
            return package.read('提交要求与待办.md').decode()
    document = Document(BytesIO(artifacts.read_bytes(result['docx_ref'])))
    return document.tables[0].cell(1, 1).text


@pytest.mark.parametrize('change', ['body_join', 'context_body_boundary', 'guidance_join'])
@pytest.mark.parametrize('compact', [False, True])
def test_native_render_semantics_change_the_actual_structure_request_body(tmp_path, change, compact):
    artifacts, original, view = fixture(tmp_path)
    changed = deepcopy(original)
    first = original['chapters'][0]['targets'][0]
    second = changed['chapters'][0]['targets'][0]
    row = second['response_table']['rows'][0]
    if change == 'body_join':
        row['source_join'] = 'paragraphs'
    elif change == 'context_body_boundary':
        row['source_excerpts'] = [*row.pop('context_source_excerpts'), *row['source_excerpts']]
    else:
        refs = next(identity for identity, atom in view.atoms.items() if atom['base']['unit_id'] == 'start')
        # Select two nonempty same-source fragments with an explicit continuation;
        # the guide is an external source reference, never an enterprise answer.
        selected = view.excerpts([refs])
        remaining = next(identity for identity, atom in view.atoms.items() if atom['base']['unit_id'] == 'end')
        selected += view.excerpts([remaining])
        for target in (first, second):
            target['response_table']['source_guidance'] = [
                {'kind': 'reference', 'source_excerpts': deepcopy(selected), 'source_join': 'continuation'}]
        second['response_table']['source_guidance'][0]['source_join'] = 'paragraphs'
    rebind(second)
    assert actual_text(artifacts, original, change == 'guidance_join') != actual_text(
        artifacts, changed, change == 'guidance_join')
    payloads = [structure._structure_payload(artifacts, outline, [], outline_phase=True)[0]
                for outline in (original, changed)]
    if compact:
        views = [compact_structure_payload(payload) for payload in payloads]
        assert all(expanded_structure(view) == expanded_structure(payload)
                   for view, payload in zip(views, payloads))
        payloads = views
    assert canonical(payloads[0]) != canonical(payloads[1])
    assert hashlib.sha256(canonical(payloads[0]).encode()).hexdigest() != hashlib.sha256(
        canonical(payloads[1]).encode()).hexdigest()
    requests = [build_request('fixture', '原结构核查。', payload, task='authoring_structure') for payload in payloads]
    assert request_digest('https://fixture.invalid/v1', requests[0]) != request_digest(
        'https://fixture.invalid/v1', requests[1])


async def test_native_structure_prompt_explains_explicit_body_context_and_join_only_for_native_layout(tmp_path):
    artifacts, native, _ = fixture(tmp_path)
    legacy = deepcopy(native)
    legacy_target = legacy['chapters'][0]['targets'][0]
    legacy_target['response_table'].pop('native_layout')
    for row in legacy_target['response_table']['rows']:
        row.pop('context_source_excerpts', None)
        row.pop('source_join', None)
    budget = RequestBudget(model='fixture', context_tokens=131072, prompts={'authoring_structure': '原结构核查。'})
    requests = []

    async def execute(_task, request, validate):
        requests.append(request)
        return validate({'findings': []})

    for outline in (native, legacy):
        await structure.review_structure(artifacts, outline, [], budget, execute, outline_phase=True)
    assert 'source_join' in requests[0]['messages'][0]['content']
    assert 'context_support_ids' in requests[0]['messages'][0]['content']
    assert 'source_join' not in requests[1]['messages'][0]['content']


def test_non_native_projection_full_compact_and_request_remain_byte_identical_to_v32(tmp_path):
    artifacts, outline, _ = fixture(tmp_path)
    target = outline['chapters'][0]['targets'][0]
    target['response_table'].pop('native_layout')
    for row in target['response_table']['rows']:
        row.pop('context_source_excerpts', None)
        row.pop('source_join', None)
    payload = structure._structure_payload(artifacts, outline, [], outline_phase=True)[0]
    assert hashlib.sha256(canonical(payload).encode()).hexdigest() == '1bab276974f5b58f3093b6d8329f7a9efd5e03c9efc64b2d3f7dde0e6d2e8ac5'
    compact = compact_structure_payload(payload)
    assert hashlib.sha256(canonical(compact).encode()).hexdigest() == 'd4626ab018f28727c65976acf35caac5a8fea6390d633c6fafc590fb74258963'
    request = build_request('fixture', '原结构核查。', payload, task='authoring_structure')
    assert hashlib.sha256(canonical(request).encode()).hexdigest() == '5e5276a1daa646df645e181a49d87b905afec57bbcd3b65fe31f14825ebd47f4'


async def test_actual_compact_review_keeps_native_join_and_context_boundary(tmp_path, monkeypatch):
    artifacts, outline, _ = fixture(tmp_path)
    budget = RequestBudget(model='fixture', context_tokens=131072, prompts={'authoring_structure': '原结构核查。'})
    original = budget.require_request

    def compact_only(request, task):
        payload = json.loads(request['messages'][1]['content'][0]['text'])
        if payload.get('structure_view') != 'compact_v2':
            raise ModelBudgetError('Synthetic scope forces the existing compact route.')
        return original(request, task)

    monkeypatch.setattr(budget, 'require_request', compact_only)

    async def execute(task, request, validate):
        payload = json.loads(request['messages'][1]['content'][0]['text'])
        assert payload['structure_view'] == 'compact_v2'
        target = payload['chapters'][0]['targets'][0]
        row = target['response_rows'][0]
        assert row['source_join'] == 'continuation'
        assert row['context_support_ids'] and row['source_support_ids']
        assert not set(row['context_support_ids']) & set(row['source_support_ids'])
        assert 'context_support_ids' in request['messages'][0]['content']
        return validate({'findings': []})

    assert await structure.review_structure(artifacts, outline, [], budget, execute, outline_phase=True) == ([], [])
