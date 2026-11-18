"""Host hierarchy numbers use native Word lists and stay out of editable business names."""

from io import BytesIO
import json
from zipfile import ZipFile

from docx import Document
from docx.oxml.ns import qn

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.word_navigation import read_docx_outline


def chapter(identity, title, *, children=(), role='body', volume=False):
    targets = ([{'target_id': identity + '-target', 'kind': 'form', 'purpose': title + '原文：________',
                 'manual_fill': True, 'rule_refs': [], 'source_refs': []}]
               if role in {'cover', 'content'} else [])
    return {'chapter_id': identity, 'title': title, 'targets': targets, 'children': list(children),
            'presentation_role': role, 'is_volume': volume}


def numbered_headings(data):
    document = Document(BytesIO(data))
    headings = [p for p in document.paragraphs if p.style.name.startswith('Heading')]
    paths, counters = [], [0] * 9
    for paragraph in headings:
        properties = paragraph._p.find(qn('w:pPr')).find(qn('w:numPr'))
        assert properties is not None, 'A body heading must carry native host numbering.'
        level = int(properties.find(qn('w:ilvl')).get(qn('w:val')))
        identity = properties.find(qn('w:numId')).get(qn('w:val'))
        numbering = document.part.numbering_part.element
        instance = next(n for n in numbering.findall(qn('w:num')) if n.get(qn('w:numId')) == identity)
        if not paths:
            for override in instance.findall(qn('w:lvlOverride')):
                counters[int(override.get(qn('w:ilvl')))] = int(
                    override.find(qn('w:startOverride')).get(qn('w:val'))) - 1
        counters[level] += 1
        counters[level + 1:] = [0] * (8 - level)
        abstract_id = instance.find(qn('w:abstractNumId')).get(qn('w:val'))
        abstract = next(n for n in numbering.findall(qn('w:abstractNum'))
                        if n.get(qn('w:abstractNumId')) == abstract_id)
        definition = abstract.findall(qn('w:lvl'))[level]
        assert definition.find(qn('w:lvlText')).get(qn('w:val')) == '.'.join(
            '%' + str(i + 1) for i in range(level + 1))
        paths.append((paragraph.text, tuple(counters[:level + 1])))
    toc = [''.join(node.text or '' for node in link.iter(qn('w:t')))
           for link in document.element.iter(qn('w:hyperlink'))]
    assert toc == ['.'.join(map(str, path)) + ' ' + title for title, path in paths]
    return paths


def test_main_and_volume_use_the_same_host_number_and_preserve_editable_titles(tmp_path):
    store = ArtifactStore(tmp_path)
    outline = {'chapters': [chapter('cover', '封面', role='cover'),
        chapter('business', '商务文件', volume=True, children=[chapter('letter', '投标函')]),
        chapter('technical', '技术文件', volume=True, children=[
            chapter('original', '原文载体', role='content'),
            chapter('parameters', '技术参数', children=[chapter('nat', '地址转换')])])]}
    result = export_authoring_document(store, outline, [])
    data = store.read_bytes(result['docx_ref'])
    main = numbered_headings(data)
    assert main == [('商务文件', (1,)), ('投标函', (1, 1)),
                    ('技术文件', (2,)), ('技术参数', (2, 1)), ('地址转换', (2, 1, 1))]
    with ZipFile(BytesIO(store.read_bytes(result['package_ref']))) as package:
        volumes = json.loads(package.read('manifest.json'))['volumes']
        assert numbered_headings(package.read(volumes[1]['package_path'])) == main[2:]
    saved = read_docx_outline(data, outline['chapters'])
    def flatten(chapters):
        for node in chapters:
            yield node
            yield from flatten(node.get('children', []))

    actual = {entry['chapter_id']: entry['title'] for entry in flatten(saved['chapters'])}
    assert {identity: actual[identity] for identity in ('business', 'technical', 'parameters', 'nat')} == {
        'business': '商务文件', 'technical': '技术文件', 'parameters': '技术参数', 'nat': '地址转换'}


def test_orphan_writing_unit_keeps_its_numbered_supplement_in_the_deliverable(tmp_path):
    from test_authoring_export import writing

    store = ArtifactStore(tmp_path)
    result = export_authoring_document(store, {'chapters': [chapter('main', '正文')]}, [
        writing('orphan-task', 'old-chapter', [], blocks=[{'kind': 'paragraph', 'text': '待重新安排的正文'}])])
    assert numbered_headings(store.read_bytes(result['docx_ref'])) == [('正文', (1,)), ('补充内容', (2,))]
