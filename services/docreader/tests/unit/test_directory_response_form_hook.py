"""Compiled response forms bind only under their frozen contract, after final IDs."""

from copy import deepcopy
from hashlib import sha256
import json

import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.directory_compile import outline_from_compiled
from docreader.tender.directory_response_forms import RESPONSE_FORM_CONTRACT, ResponseFormView
from test_directory_response_forms import source_case, synthetic_response


def response_case(tmp_path, contract=RESPONSE_FORM_CONTRACT):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled, selection, _ = source_case(artifacts)
    view = ResponseFormView(artifacts, compiled, documents, selection, fixed)
    compiled['_response_form_plans'] = [view.accept(synthetic_response(view.payload))]
    if contract is not None:
        compiled['_response_form_contract'] = contract
    return fixed, compiled


def test_compile_binds_response_rows_to_final_target_ids_for_each_subject(tmp_path):
    fixed, compiled = response_case(tmp_path)
    chapter = compiled['chapters'][0]
    chapter['subjects'] = [{'role': role} for role in ('投标人', '制造商')]
    original = deepcopy(chapter)
    targets = outline_from_compiled(fixed, compiled)['chapters'][0]['targets']
    assert chapter == original
    assert {target['subject'] for target in targets} == {'投标人', '制造商'}
    rows = []
    for target in targets:
        table = target['response_table']
        assert table['rows'] and target['target_id'].startswith('chapter-1:target-')
        assert all(row['row_id'].startswith(target['target_id'] + ':row-') for row in table['rows'])
        rows.extend(row['row_id'] for row in table['rows'])
    assert len(rows) == len(set(rows))


@pytest.mark.parametrize('contract', [None, 'unknown-future-contract'])
def test_compile_without_exact_response_contract_keeps_old_materialization(tmp_path, contract):
    fixed, compiled = response_case(tmp_path, contract)
    before = deepcopy(compiled)
    chapter = outline_from_compiled(fixed, compiled)['chapters'][0]
    targets = chapter['targets']
    assert all(target.get('response_table') is None for target in targets)
    assert chapter['chapter_id'] == 'chapter-1'
    assert [target['target_id'] for target in targets] == ['chapter-1:target-1']
    rows = [{'document_id': 'source', 'page': 0, 'line': 0, 'text': '逐条评审响应表'},
            {'document_id': 'source', 'page': 0, 'line': 1, 'text': '投标人应逐条列出以下评审要求原文。'}]
    purpose = '\n'.join(row['text'] for row in rows)
    assert targets[0]['purpose'] == purpose
    assert compiled == {**before, '_source_issue_cases': {}, '_source_renderings': [{
        'target_id': 'chapter-1:target-1', 'chapter_id': 'chapter-1', 'rows': rows,
        'rows_sha256': sha256(json.dumps(rows, ensure_ascii=False, sort_keys=True,
                                        separators=(',', ':')).encode()).hexdigest(),
        'generated': False, 'purpose_sha256': sha256(purpose.encode()).hexdigest()}]}


def test_compile_includes_named_unbound_response_form_issue(tmp_path):
    fixed, compiled = response_case(tmp_path)
    compiled['_response_form_plans'][0]['chapter_ids'] = ['missing-source-node']
    outline = outline_from_compiled(fixed, compiled)
    assert any(issue['code'] == 'outline_directory_response_form_unbound' for issue in outline['issues'])
