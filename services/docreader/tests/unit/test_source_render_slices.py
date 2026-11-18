"""Per-window whitespace rendering retains original coordinates for decorations."""

from copy import deepcopy
from hashlib import sha256

import pytest

from docreader.tender.directory_compile import compile_directory, outline_from_compiled
from docreader.tender.source_presentation import project_decorations, _rows_sha
from test_directory_compile import item, node, plan
from test_source_presentation import fixture


def compiled_windows(tmp_path):
    _, snapshot, _, _, _, _ = fixture(tmp_path)
    pages = ['Original form\n  Team requirements\n  Manager: 1; experience: 5 years\n ',
             '                                TENDER HEADER\n\nCISP; resident staff: 3\n  Service: 5x10 hours; 9 months  \n  ']
    documents = [{'document_id': 'document', 'role': 'main', 'pages': pages}]
    windows = [{'document_id': 'document', 'page': 0, 'start_line': 1, 'end_line': 3},
               {'document_id': 'document', 'page': 1, 'start_line': 0, 'end_line': 4}]
    raw = plan([{**node('form', 0, 0, document_id='document'), 'body_windows': [
        {'document_id': 'document', 'page': 0, 'start_line': 0, 'end_line': 0}]}],
        [item('form', clause=windows)])
    compiled = compile_directory(raw, documents)
    outline = outline_from_compiled(snapshot, compiled)
    line = pages[1].splitlines()[0]
    fact = {'kind': 'source_decoration', 'fact_id': 'reviewed-header', 'label': 'Exact header',
        'document_id': 'document', 'original_ref': {'original': 'unchanged'}, 'review_evidence_ref': {},
        '_fragments': [{'page': 1, 'line': 0, 'start': 0, 'end': len(line), 'text': line,
            'line_text': line, 'text_sha256': sha256(line.encode()).hexdigest()}],
        'context_approvals': [{'owner_heading': compiled['chapters'][0]['heading'],
            'context_kind': 'clause', 'context_window': windows[1]}]}
    return compiled, outline, fact, windows


def test_per_window_trim_projection_preserves_complete_team_conditions_and_original_rows(tmp_path):
    compiled, outline, fact, windows = compiled_windows(tmp_path)
    chapter_before = deepcopy(compiled['chapters'][0])
    target = outline['chapters'][0]['targets'][1]
    before = target['purpose']
    assert before == ('Team requirements\n  Manager: 1; experience: 5 years\nTENDER HEADER\n\n'
                      'CISP; resident staff: 3\n  Service: 5x10 hours; 9 months')
    project_decorations(outline, compiled, [fact])
    assert target['purpose'] == before.replace('TENDER HEADER', '')
    assert compiled['_reviewed_source_fact_applications'][0]['state'] == 'presentation_excluded'
    assert compiled['chapters'][0] == chapter_before
    assert compiled['chapters'][0]['clause_windows'] == windows
    rows = compiled['_source_renderings'][1]['rows']
    assert rows[0]['text'] == '  Team requirements'
    assert rows[2]['text'] == ' ' and rows[2]['rendered'] is False
    assert rows[3]['text'].startswith(' ' * 32) and rows[3]['render_start'] == 32


@pytest.mark.parametrize('mutation', ['no_approval', 'stale_line', 'invalid_slice'])
def test_window_slices_never_bypass_source_or_context_guards(tmp_path, mutation):
    compiled, outline, fact, _ = compiled_windows(tmp_path)
    if mutation == 'no_approval':
        fact['context_approvals'] = []
    else:
        origin = compiled['_source_renderings'][1]
        row = origin['rows'][3]
        if mutation == 'stale_line':
            row['text'] = row['text'].replace('TENDER', 'UNSEEN')
        else:
            row['render_end'] = len(row['text']) + 1
        origin['rows_sha256'] = _rows_sha(origin['rows'])
    before = deepcopy(outline['chapters'])
    project_decorations(outline, compiled, [fact])
    assert outline['chapters'] == before
    assert compiled['_reviewed_source_fact_applications'][0]['state'] == 'unresolved'
