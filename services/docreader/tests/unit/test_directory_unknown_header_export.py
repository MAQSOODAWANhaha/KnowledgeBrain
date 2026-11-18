"""An unconfirmed native header cannot bypass the material merge check at export."""

from docx.oxml.ns import qn

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender import directory_compile as directory

from test_directory_material_headers import header_issues, native_case, word


def test_unknown_native_header_does_not_repeat_a_partial_merged_header_in_real_word(tmp_path):
    artifacts = ArtifactStore(tmp_path)
    fixed, documents, compiled = native_case(artifacts, native_headers=[
        {'row': 0, 'column': 0, 'kind': 'column_header'}])
    catalog = directory.table_header_catalog(compiled, documents)
    assert len(catalog) == 1
    directory.apply_table_headers(compiled, documents, catalog, [
        {'table_id': catalog[0]['table_id'], 'count': None}])
    assert header_issues(compiled)
    _outline, document = word(artifacts, fixed, compiled)
    assert len(document.tables) == 1
    assert all(row._tr.trPr is None or row._tr.trPr.find(qn('w:tblHeader')) is None
               for row in document.tables[0].rows)
    assert document.tables[0].cell(0, 0).text == '项目'
    assert document.tables[0].cell(2, 0).text == '资产'
    assert any('在 Word' in paragraph.text and '重复标题行' in paragraph.text
               for paragraph in document.paragraphs)
