"""A submission guide must describe the rows in the currently placed original."""

from copy import deepcopy
from io import BytesIO
from zipfile import ZipFile

import pytest

from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.outline_edit import apply_outline_edit
from test_response_form_row_roles import role_case, role_answer, bound_outline


@pytest.mark.parametrize('change', ['unplaced', 'reordered'])
def test_export_guide_reports_stale_row_roles_instead_of_old_confirmed_row_order(tmp_path, change):
    artifacts, fixed, compiled, _, view = role_case(tmp_path)
    outline = bound_outline(artifacts, fixed, compiled, view, role_answer(view))
    if change == 'unplaced':
        chapter = outline['chapters'][0]
        outline = apply_outline_edit(outline, [{'chapter_id': chapter['chapter_id'], 'title': chapter['title'],
                                               'target_ids': [], 'children': []}])
        assert outline['unplaced_targets']
    else:
        outline = deepcopy(outline)
        outline['chapters'][0]['targets'][0]['response_table']['rows'].reverse()
    result = export_authoring_document(artifacts, outline, [])
    with ZipFile(BytesIO(artifacts.read_bytes(result['package_ref']))) as package:
        guide = package.read('提交要求与待办.md').decode()
    assert '原式行责任记录与当前响应位置不一致' in guide
    assert '按来源展开原式。' not in guide
    assert '第1行：投标人逐项应答条款' not in guide
