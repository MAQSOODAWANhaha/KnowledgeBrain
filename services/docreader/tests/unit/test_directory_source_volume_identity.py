"""A source scope can already own its container as an adopted chapter."""

from copy import deepcopy
from hashlib import sha256
from io import BytesIO
import json
from zipfile import ZipFile

from docx import Document

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.directory_compile import compile_directory, outline_from_compiled
from test_directory_compile import docs, item, node, plan, point, window


SNAPSHOT = {'schema_version': 2, 'run_id': 'source-volumes', 'source_version': 1, 'domain_revision': 1}


def source_container(*, detached=False):
    documents = docs(['交付分册\n容器固定正文', '第一材料\n第一原式', '第二材料\n第二原式'])
    raw = plan([
        {**node('volume', 0, 0), 'body_windows': [window(0, 0, 1)]},
        {**node('first', 1, 0, parent='volume'), 'body_windows': [window(1, 0, 1)]},
        {**node('second', 2, 0, parent=None if detached else 'volume'), 'body_windows': [window(2, 0, 1)]},
    ], [item('volume'), item('first'), item('second')], title=point(0, 0))
    return documents, raw


def test_adopted_source_scope_container_is_one_volume_with_its_own_body_and_children():
    documents, raw = source_container()
    compiled = compile_directory(raw, documents)
    before = deepcopy(compiled)
    outline = outline_from_compiled(SNAPSHOT, compiled)

    assert len(outline['chapters']) == 1
    volume = outline['chapters'][0]
    assert volume['is_volume'] and volume['title'] == '交付分册'
    assert [chapter['title'] for chapter in volume['children']] == ['第一材料', '第二材料']
    assert [target['purpose'] for target in volume['targets']] == ['交付分册\n容器固定正文']
    renderings = []
    for page, (chapter, identity) in enumerate(zip([volume, *volume['children']],
                                                 ['chapter-1', 'chapter-1.1', 'chapter-1.2'])):
        assert chapter['chapter_id'] == identity
        assert [target['target_id'] for target in chapter['targets']] == [identity + ':target-1']
        purpose = documents[0]['pages'][page]
        assert chapter['targets'][0]['purpose'] == purpose
        rows = [{'document_id': 'tender', 'page': page, 'line': line, 'text': text}
                for line, text in enumerate(purpose.splitlines())]
        renderings.append({'target_id': identity + ':target-1', 'chapter_id': identity, 'rows': rows,
            'rows_sha256': sha256(json.dumps(rows, ensure_ascii=False, sort_keys=True,
                                            separators=(',', ':')).encode()).hexdigest(),
            'generated': False, 'purpose_sha256': sha256(purpose.encode()).hexdigest()})
    assert compiled == {**before, '_source_issue_cases': {}, '_source_renderings': renderings}


def test_same_source_group_reuses_container_for_detached_members_without_duplicate_wrapper():
    documents, raw = source_container(detached=True)
    compiled = compile_directory(raw, documents)
    volume = outline_from_compiled(SNAPSHOT, compiled)['chapters'][0]

    assert volume['is_volume']
    assert [chapter['title'] for chapter in volume['children']] == ['第一材料', '第二材料']
    assert [chapter['chapter_id'] for chapter in volume['children']] == ['chapter-1.1', 'chapter-1.2']
    assert all(not chapter['is_volume'] for chapter in volume['children'])


def test_a_display_alias_cannot_remove_the_source_container_identity():
    documents, raw = source_container()
    compiled = compile_directory(raw, documents)
    compiled['chapters'][0]['display_title'] = '申请材料'
    volume = outline_from_compiled(SNAPSHOT, compiled)['chapters'][0]
    assert volume['is_volume'] and volume['title'] == '申请材料'
    assert len(volume['children']) == 2


def test_equal_source_names_at_different_coordinates_do_not_make_the_form_a_container():
    documents = docs(['交付分册', '交付分册\n这是一份同名原式'])
    compiled = compile_directory(plan([
        {**node('form', 1, 0), 'body_windows': [window(1, 0, 1)]}],
        [item('form')], title=point(0, 0)), documents)
    volume = outline_from_compiled(SNAPSHOT, compiled)['chapters'][0]
    assert volume['is_volume'] and not volume['targets']
    assert len(volume['children']) == 1
    assert volume['children'][0]['title'] == '交付分册'
    assert not volume['children'][0]['is_volume']
    assert volume['children'][0]['targets'][0]['purpose'] == '交付分册\n这是一份同名原式'


def test_equal_group_names_from_different_source_anchors_remain_separate_volumes():
    documents = docs(['交付分册\n甲固定正文', '交付分册\n乙固定正文'])
    raw = plan([
        {**node('a', 0, 0), 'body_windows': [window(0, 0, 1)]},
        {**node('b', 1, 0), 'body_windows': [window(1, 0, 1)]}], [])
    raw['scopes'] = [{'title': point(0, 0), 'items': [item('a')]},
                     {'title': point(1, 0), 'items': [item('b')]}]
    chapters = outline_from_compiled(SNAPSHOT, compile_directory(raw, documents))['chapters']
    assert len(chapters) == 2 and all(chapter['is_volume'] for chapter in chapters)
    assert [chapter['targets'][0]['purpose'] for chapter in chapters] == [
        '交付分册\n甲固定正文', '交付分册\n乙固定正文']


def test_untitled_scope_and_ordinary_parent_do_not_gain_a_volume_from_their_names():
    documents, raw = source_container()
    raw['scopes'][0]['title'] = None
    outline = outline_from_compiled(SNAPSHOT, compile_directory(raw, documents))
    assert len(outline['chapters']) == 1
    assert not outline['chapters'][0]['is_volume']
    assert len(outline['chapters'][0]['children']) == 2


def test_an_adopted_parent_prevents_a_nested_scope_node_becoming_an_independent_volume():
    documents, raw = source_container()
    raw['scopes'] = [{'title': None, 'items': [item('volume')]},
                     {'title': point(1, 0), 'items': [item('first')]},
                     {'title': None, 'items': [item('second')]}]
    chapter = outline_from_compiled(SNAPSHOT, compile_directory(raw, documents))['chapters'][0]
    assert not chapter['is_volume']
    assert len(chapter['children']) == 2
    assert all(not child['is_volume'] for child in chapter['children'])


def test_matching_names_and_page_coordinates_from_two_documents_do_not_merge_volumes():
    documents = docs(['交付分册\n申报人原文'],
                     {'document_id': 'attachment', 'role': 'attachment', 'pages': ['交付分册\n制造商原文']})
    raw = plan([
        {**node('applicant', 0, 0), 'body_windows': [window(0, 0, 1)]},
        {**node('manufacturer', 0, 0, document_id='attachment'),
         'body_windows': [window(0, 0, 1, document_id='attachment')]}], [])
    raw['scopes'] = [{'title': point(0, 0), 'items': [item('applicant')]},
                     {'title': point(0, 0, document_id='attachment'), 'items': [item('manufacturer')]}]
    chapters = outline_from_compiled(SNAPSHOT, compile_directory(raw, documents))['chapters']
    assert len(chapters) == 2 and all(chapter['is_volume'] for chapter in chapters)
    assert [chapter['targets'][0]['purpose'] for chapter in chapters] == [
        '交付分册\n申报人原文', '交付分册\n制造商原文']


def test_matching_source_cover_and_content_nodes_do_not_become_volumes():
    for presentation in ('cover', 'content'):
        documents = docs(['文书封面\n固定信息'])
        raw = plan([{**node('cover', 0, 0), 'body_windows': [window(0, 0, 1)]}],
                   [item('cover', presentation=presentation)], title=point(0, 0))
        chapter = outline_from_compiled(SNAPSHOT, compile_directory(raw, documents))['chapters'][0]
        assert not chapter['is_volume']
        assert chapter['presentation_role'] == presentation


def test_reused_source_container_exports_one_complete_subtree_as_an_independent_volume(tmp_path):
    documents, raw = source_container(detached=True)
    outline = outline_from_compiled(SNAPSHOT, compile_directory(raw, documents))
    artifacts = ArtifactStore(tmp_path / 'artifacts')
    exported = export_authoring_document(artifacts, outline, [])
    with ZipFile(BytesIO(artifacts.read_bytes(exported['package_ref']))) as package:
        manifest = json.loads(package.read('manifest.json'))
    assert len(manifest['volumes']) == 1
    volume = manifest['volumes'][0]
    word = Document(BytesIO(artifacts.read_bytes(volume['artifact_ref'])))
    text = '\n'.join(paragraph.text for paragraph in word.paragraphs)
    assert text.count('容器固定正文') == 1
    assert text.count('第一原式') == text.count('第二原式') == 1
    assert text.index('第一原式') < text.index('第二原式')
    with ZipFile(BytesIO(artifacts.read_bytes(exported['package_ref']))) as package:
        assert package.read(volume['package_path']) == artifacts.read_bytes(volume['artifact_ref'])
