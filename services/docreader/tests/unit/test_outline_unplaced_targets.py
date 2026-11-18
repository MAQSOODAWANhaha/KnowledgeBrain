"""Removing source-only responses must retain their immutable source candidates."""

from copy import deepcopy
from io import BytesIO
import json
from types import SimpleNamespace
from zipfile import ZipFile

from docx import Document
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.integrations.web import WebError, _outline_request
from docreader.tender.authoring_coverage import build_ledger, coverage_headline, settle_coverage
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.authoring_requirements import preserve_requirements
from docreader.tender.outline_edit import apply_outline_edit
from docreader.tender.schema import AuthoringOutline

from test_authoring_export import grid, source_ref
from test_outline_edit import outline


def source_only_outline():
    base = outline()
    base['chapters'][0]['targets'][0]['rule_refs'] = []
    return base


def test_source_only_removed_form_keeps_full_candidate_and_pending_denominator():
    base = source_only_outline()
    before = deepcopy(base)
    original = base['chapters'][0]['targets'][0]
    changed = apply_outline_edit(base, [{'chapter_id': 'b', 'title': '保留', 'children': []}])
    assert changed['chapters'][0]['targets'] == []
    assert changed['unplaced_targets'] == [original]
    assert base == before
    entries = build_ledger(changed, {'rules': []})
    assert len(entries) == len(build_ledger(base)) == 1
    assert entries[0]['target_id'] == original['target_id']
    assert entries[0]['resolution'] == 'pending'
    assert entries[0]['coverage_id'] == build_ledger(base)[0]['coverage_id']
    settled = settle_coverage(entries, [], [], [])
    assert settled[0]['gap_codes'] == ['coverage_target_missing']
    assert coverage_headline({'entries': settled})['unreviewed'] == 1


def test_removed_candidate_survives_resave_without_catalog_and_clears_only_when_reassigned():
    base = source_only_outline()
    original = deepcopy(base['chapters'][0]['targets'][0])
    tree = [{'chapter_id': 'b', 'title': '保留', 'children': []}]
    changed = apply_outline_edit(base, tree)
    repeated = apply_outline_edit(changed, tree)
    assert repeated['unplaced_targets'] == [original]
    assert repeated == changed
    restored = apply_outline_edit(repeated, [{'chapter_id': 'b', 'title': '恢复位置',
        'target_ids': [original['target_id']], 'children': []}])
    assert restored['chapters'][0]['targets'] == [original]
    assert restored['unplaced_targets'] == []
    assert not any(issue['code'] == 'outline_edit_target_removed' for issue in restored['issues'])
    assert build_ledger(restored)[0]['resolution'] == 'manual'


def test_server_owned_candidate_backfills_an_old_empty_snapshot():
    base = source_only_outline()
    original = deepcopy(base['chapters'][0]['targets'][0])
    base['chapters'][0]['targets'] = []
    base['unplaced_targets'] = [original]
    changed = apply_outline_edit(base, [{'chapter_id': 'b', 'title': '保留', 'children': []}],
                                 target_catalog=[original])
    assert changed['unplaced_targets'] == [original]
    assert build_ledger(changed)[0]['resolution'] == 'pending'


def test_removed_table_rows_keep_each_source_row_in_the_denominator():
    base = source_only_outline()
    original = base['chapters'][0]['targets'][0]
    excerpt = {'source_ref': original['source_refs'][0], 'selectors': [
        {'range_id': 'row-range', 'source_range_id': 'row-range', 'start': 0, 'end': 1}]}
    original['response_table'] = {'rows': [
        {'row_id': 'first', 'rule_refs': [], 'source_excerpts': [excerpt]},
        {'row_id': 'second', 'rule_refs': [], 'source_excerpts': [excerpt]}]}
    changed = apply_outline_edit(base, [])
    entries = build_ledger(changed)
    assert [entry['row_id'] for entry in entries] == ['first', 'second']
    assert all(entry['resolution'] == 'pending' for entry in entries)


def test_client_cannot_supply_owned_unplaced_target_snapshots():
    with pytest.raises(WebError, match='响应|来源|快照'):
        _outline_request({'expected_revision_id': None, 'source_execution_id': 'source',
                          'chapters': [], 'unplaced_targets': []})


def test_old_outline_contract_defaults_to_no_unplaced_candidates():
    base = source_only_outline()
    assert AuthoringOutline.model_validate(base).unplaced_targets == []


@pytest.mark.parametrize('with_requirements', [True, False])
def test_unplaced_form_reaches_word_and_zip_without_recreating_its_chapter(tmp_path, with_requirements):
    artifacts = ArtifactStore(tmp_path)
    base = source_only_outline()
    original = base['chapters'][0]['targets'][0]
    reference = source_ref(artifacts, grid=grid())
    original['source_refs'] = [reference]
    original['template_refs'] = [{'source_ref': reference, 'selectors': [], 'header_row_count': 1}]
    changed = apply_outline_edit(base, [{'chapter_id': 'b', 'title': '保留', 'children': []}])
    context = SimpleNamespace(availability_issues=[], payload={}, rules={},
                              rule_origin='source_only', input=changed['authoring_input'])
    requirements = preserve_requirements(context, changed)
    assert requirements['unplaced_targets'] == [original]
    result = export_authoring_document(artifacts, changed, [],
                                       requirements=requirements if with_requirements else None)
    doc = Document(BytesIO(artifacts.read_bytes(result['docx_ref'])))
    text = '\n'.join(paragraph.text for paragraph in doc.paragraphs)
    assert '尚未安排的原响应' in text and original['purpose'] in text
    assert '大纲编排' in text and '响应安排' in text
    assert not doc.tables
    assert any(gap['code'] == 'export_target_unassigned' and gap['target_id'] == original['target_id']
               for gap in result['gaps'])
    with ZipFile(BytesIO(artifacts.read_bytes(result['package_ref']))) as archive:
        retained = (json.loads(archive.read('招标要求底稿.json'))['unplaced_targets']
                    if with_requirements else json.loads(archive.read('未安排响应.json')))
    assert retained == [original]
    assert retained[0]['template_refs'][0]['source_ref'] == reference
    raw = artifacts.read_json(artifacts.read_json(reference['source_index_ref'])['raw_ref'])
    assert [cell['text'] for cell in raw['units'][0]['grid']['cells']].count('') == 2
