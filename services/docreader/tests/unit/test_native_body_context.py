"""Native source bodies and their context stay distinct through structure and Word."""
from copy import deepcopy
from io import BytesIO
from zipfile import ZipFile

from docx import Document
import pytest
from pydantic import ValidationError

from docreader.integrations.artifacts import ArtifactIntegrityError
from docreader.tender.authoring_excerpts import source_excerpts_text
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.authoring_structure import _structure_payload
from docreader.tender.evidence.references import fragment, selector
from docreader.tender.schema import AuthoringOutline, ResponseTarget
from docreader.tender.source_body import bind_source_body
from test_authoring_templates import create_source
from test_authoring_export import writing


CONTRACT = 'source_bound_native_body_v1'


def source_case(tmp_path):
    grid = {'row_count': 1, 'column_count': 3, 'cells': [
        {'row': 0, 'column': column, 'row_span': 1, 'col_span': 1, 'text': text}
        for column, text in enumerate(('建设方案评价', '5', '投标人应提交完整实施方案。'))]}
    artifacts, index_ref, index = create_source(tmp_path, [{'grid': grid}])
    base = index.ranges[0]

    def cell(column):
        value = next(c for c in base['cell_intervals'] if c['column'] == column)
        return {'source_ref': {'source_index_ref': index_ref, 'unit_id': base['unit_id']},
                'selectors': [selector(fragment(base, value['start'], value['end']))]}

    body, context = [cell(2)], [cell(0), cell(1)]
    target = {'target_id': 'chapter-1:target-1', 'kind': 'narrative',
        'purpose': source_excerpts_text(artifacts, body), 'rule_refs': [], 'source_refs': [],
        'source_excerpts': body, 'source_context_excerpts': context,
        'source_body_contract': CONTRACT, 'manual_fill': True, 'support_origins': ['tender']}
    frozen = artifacts.put_json({'documents': [{**index.document, 'source_index_ref': index_ref, 'role': 'main'}]})
    outline = {'schema_version': 2, 'authoring_input': {'schema_version': 2, 'run_id': 'native-body',
        'source_version': 1, 'domain_revision': 1, 'frozen_ref': frozen},
        'chapters': [{'chapter_id': 'chapter-1', 'title': '实施方案', 'targets': [target], 'children': []}],
        'constraint_rule_refs': [], 'submission_refs': [], 'search_intents': [], 'issues': []}
    target = bind_source_body(artifacts, outline['authoring_input'], target)
    outline['chapters'][0]['targets'][0] = target
    return artifacts, outline, target


def test_default_target_wire_does_not_acquire_native_context_fields():
    legacy = {'target_id': 't', 'kind': 'narrative', 'purpose': '编制方案', 'rule_refs': [], 'source_refs': []}
    assert ResponseTarget.model_validate(legacy).model_dump(exclude_unset=True) == legacy


def test_explicit_native_body_and_context_survive_public_outline_roundtrip(tmp_path):
    _, outline, target = source_case(tmp_path)
    saved = AuthoringOutline.model_validate(outline).model_dump(exclude_unset=True)
    assert saved['chapters'][0]['targets'][0] == target
    assert '建设方案评价' not in target['purpose'] and '5' not in target['purpose']


@pytest.mark.parametrize('bad', ['no_contract', 'no_binding', 'template', 'overlap'])
def test_native_context_requires_explicit_source_body_authority(tmp_path, bad):
    _, _, target = source_case(tmp_path)
    if bad == 'no_contract':
        target.pop('source_body_contract')
    elif bad == 'no_binding':
        target.pop('source_body_ref')
    elif bad == 'template':
        target['template_refs'] = [{**deepcopy(target['source_excerpts'][0]), 'selectors': []}]
    else:
        target['source_context_excerpts'] = deepcopy(target['source_excerpts'])
    with pytest.raises(ValidationError):
        ResponseTarget.model_validate(target)


def test_source_authority_does_not_change_the_enterprise_answer_mode(tmp_path):
    artifacts, outline, target = source_case(tmp_path)
    target.update(manual_fill=False, support_origins=['knowledge'])
    assert ResponseTarget.model_validate(target).manual_fill is False
    result = export_authoring_document(artifacts, outline, [])
    paragraphs = [p.text for p in Document(BytesIO(artifacts.read_bytes(result['docx_ref']))).paragraphs]
    assert '投标人应提交完整实施方案。' in paragraphs
    assert '建设方案评价\n5' in paragraphs
    assert target['support_origins'] == ['knowledge']


def test_word_keeps_exact_body_context_and_one_blank_without_expanding_the_source_row(tmp_path):
    artifacts, outline, target = source_case(tmp_path)
    # Exercise the legacy branch which would widen an exact cell when rules exist.
    target['rule_refs'] = [{'item_ref': {'artifact_ref': artifacts.put_json({}), 'item_id': 'rule'}}]
    units = [writing('manual', 'chapter-1', [target['target_id']], blocks=[
        {'kind': 'paragraph', 'text': '我方响应：________（人工填写）'}])]
    result = export_authoring_document(artifacts, outline, units)
    word = Document(BytesIO(artifacts.read_bytes(result['docx_ref'])))
    paragraphs = [p.text for p in word.paragraphs]
    assert word.tables == []
    body_at = paragraphs.index('投标人应提交完整实施方案。')
    context_at = paragraphs.index('原文上下文（非企业应答）：')
    answer_at = paragraphs.index('我方响应：________（人工填写）')
    assert body_at < context_at < answer_at
    assert '建设方案评价\n5' in paragraphs[context_at:answer_at]
    assert paragraphs.count('我方响应：________（人工填写）') == 1
    with ZipFile(BytesIO(artifacts.read_bytes(result['package_ref']))) as package:
        assert package.read('投标文件.docx') == artifacts.read_bytes(result['docx_ref'])


def test_structure_review_receives_body_and_context_as_separate_exact_supports(tmp_path):
    artifacts, outline, _ = source_case(tmp_path)
    payload, gaps = _structure_payload(artifacts, outline, [], outline_phase=True)
    target = payload['chapters'][0]['targets'][0]
    supports = {s['support_id']: s['text'] for s in payload['supports']}
    assert [supports[s] for s in target['source_support_ids']] == ['投标人应提交完整实施方案。']
    assert [supports[s] for s in target['source_context_support_ids']] == ['建设方案评价', '5']
    assert target['source_body_contract'] == CONTRACT
    assert not gaps


def test_missing_native_context_cannot_silently_produce_a_successful_word(tmp_path):
    artifacts, outline, target = source_case(tmp_path)
    target['source_context_excerpts'][0]['selectors'][0]['source_range_id'] = 'missing'
    with pytest.raises(ArtifactIntegrityError):
        export_authoring_document(artifacts, outline, [])


@pytest.mark.parametrize('consumer', ['word', 'structure'])
@pytest.mark.parametrize('corruption', ['purpose', 'body_selector', 'frozen_source'])
def test_source_body_is_verified_before_any_consumer_accepts_it(tmp_path, consumer, corruption):
    artifacts, outline, target = source_case(tmp_path)
    if corruption == 'purpose':
        target['purpose'] += '5分，建设方案评价。'
    elif corruption == 'body_selector':
        target['source_excerpts'][0]['selectors'][0]['source_range_id'] = 'missing'
    else:
        outline['authoring_input']['frozen_ref'] = artifacts.put_json({'documents': []})
    with pytest.raises(ArtifactIntegrityError):
        if consumer == 'word':
            export_authoring_document(artifacts, outline, [])
        else:
            _structure_payload(artifacts, outline, [], outline_phase=True)
