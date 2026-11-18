from io import BytesIO
import json
from zipfile import ZipFile

from docx import Document
import pytest

from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.authoring_pipeline import _request_directory_details
from docreader.tender.directory_compile import outline_from_compiled
from docreader.tender.native_requirement_sources import CONTRACT
from test_directory_compile import fixture_titles
from test_native_requirement_sources import budget
from test_reviewed_source_facts import fixture, load


async def test_joint_classification_over_budget_preserves_named_original_and_blank_in_word(tmp_path, monkeypatch):
    data = fixture(tmp_path)
    artifacts, snapshot, documents, manifest, compiled, _ = data
    picked = manifest['facts'][-1]
    picked['label'] = '保密方案的原文职责'
    manifest['facts'] = [picked]
    facts = load(data)
    request_budget = budget()
    require_request = request_budget.require_request
    measured = []

    def cap_classification_output(request, task=None):
        admission = require_request(request, task)
        if task == 'directory_classification':
            measured.append(task)
            return {**admission, 'output_reserve': 1}
        return admission

    monkeypatch.setattr(request_budget, 'require_request', cap_classification_output)
    sent = []

    async def execute(task, request, validate):
        sent.append(str(task))
        payload = json.loads(request['messages'][1]['content'][0]['text'])
        if task == 'directory_material':
            return validate({'assignments': [], 'chapter_titles': fixture_titles(payload),
                             'carrier_dispositions': [], 'source_groups': []})
        assert task == 'directory_requirement'
        spans = [r for row in payload['source_scope'] for r, _ in row.get('lines', [])]
        reviewed = set(payload.get('reviewed_body_refs', []))
        dispositions = [{'kind': 'already_carried', 'refs': sorted(reviewed)}] if reviewed else []
        remaining = [ref for ref in spans if ref not in reviewed]
        if remaining:
            dispositions.append({'kind': 'reference', 'refs': remaining})
        return validate({'assignments': [], 'native_dispositions': dispositions, 'native_assignments': []})

    await _request_directory_details(compiled, documents, request_budget, execute, snapshot, artifacts=artifacts,
        source_carrier_contract='source_carriers_closed_v1', native_requirement_contract=CONTRACT,
        reviewed_source_facts=facts)
    assert measured == ['directory_classification', 'directory_classification']
    assert sent == ['directory_material', 'directory_requirement']
    record, = compiled['_reviewed_source_fact_applications']
    assert record['state'] == 'unresolved'
    outline = outline_from_compiled(snapshot, compiled, artifacts=artifacts)
    delivery = export_authoring_document(artifacts, outline, [])
    word = artifacts.read_bytes(delivery['docx_ref'])
    paragraphs = [p.text for p in Document(BytesIO(word)).paragraphs]
    review_from = paragraphs.index('适用性与来源待核')
    named, = [i for i, text in enumerate(paragraphs) if '保密方案的原文职责' in text]
    assert named > review_from
    assert paragraphs[named].endswith('应制定保密方案\n包括管理与应急')
    assert paragraphs[named + 1] == '处理结果：________（人工核定）'
    assert any('部分来源分类待核' in p for p in paragraphs[review_from:])
    with ZipFile(BytesIO(artifacts.read_bytes(delivery['package_ref']))) as package:
        assert package.read('投标文件.docx') == word


@pytest.mark.parametrize('code', [
    'outline_directory_native_source_unavailable', 'outline_directory_classification_rejected',
])
def test_pending_outlet_accepts_saved_legacy_outline_issue(tmp_path, code):
    artifacts, _, _, _, _, _ = fixture(tmp_path)
    issue = '已保存的大纲仍需人工核对该来源。'
    delivery = export_authoring_document(artifacts, {'chapters': [], 'issues': [{'code': code, 'detail': issue}]}, [])
    doc = Document(BytesIO(artifacts.read_bytes(delivery['docx_ref'])))
    paragraphs = [p.text for p in doc.paragraphs]
    assert paragraphs[paragraphs.index(issue) + 1] == '处理结果：________（人工核定）'
