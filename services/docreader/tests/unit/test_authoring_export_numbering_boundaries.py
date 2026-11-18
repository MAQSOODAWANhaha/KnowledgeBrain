"""Full host labels survive Word's nine-level list limit and auxiliary appendices."""

from io import BytesIO
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from zipfile import ZipFile

from docx import Document
from docx.oxml.ns import qn
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender.word_navigation import read_docx_outline

from test_authoring_export import chapter, target


def deep_outline():
    branch = chapter('sixteen', '最深业务项')
    for depth in range(15, 10, -1):
        branch = chapter(f'depth-{depth}', f'业务层{depth}', children=[branch])
    first_nine = chapter('nine-a', '第九层甲', children=[
        chapter('ten-a', '第十层甲', children=[branch]), chapter('ten-b', '第十层乙')])
    second_nine = chapter('nine-b', '回到第九层', children=[chapter('ten-c', '后续第十层')])
    branch = chapter('eight', '第八层', children=[first_nine, second_nine])
    for depth in range(7, 1, -1):
        branch = chapter(f'depth-{depth}', f'业务层{depth}', children=[branch])
    return {'chapters': [{**chapter('first', '商务文件'), 'is_volume': True},
                         {**chapter('second', '技术文件', children=[branch]), 'is_volume': True}]}


def expected_paths(chapters, prefix=()):
    result = []
    for ordinal, value in enumerate(chapters, 1):
        path = (*prefix, ordinal)
        result.append((value['title'], path))
        result.extend(expected_paths(value['children'], path))
    return result


def native_labels(data):
    document = Document(BytesIO(data))
    numbering = document.part.numbering_part.element
    instances = {n.get(qn('w:numId')): n for n in numbering.findall(qn('w:num'))}
    abstracts = {n.get(qn('w:abstractNumId')): n for n in numbering.findall(qn('w:abstractNum'))}
    states, labels = {}, []
    for paragraph in document.paragraphs:
        if not paragraph.style.name.startswith('Heading'):
            continue
        properties = paragraph._p.find(f"{qn('w:pPr')}/{qn('w:numPr')}")
        assert properties is not None
        level = int(properties.find(qn('w:ilvl')).get(qn('w:val')))
        identity = properties.find(qn('w:numId')).get(qn('w:val'))
        instance = instances[identity]
        if identity not in states:
            states[identity] = [0] * 9
            for override in instance.findall(qn('w:lvlOverride')):
                states[identity][int(override.get(qn('w:ilvl')))] = int(
                    override.find(qn('w:startOverride')).get(qn('w:val'))) - 1
        values = states[identity]
        values[level] += 1
        values[level + 1:] = [0] * (8 - level)
        abstract = abstracts[instance.find(qn('w:abstractNumId')).get(qn('w:val'))]
        definition = next(n for n in abstract.findall(qn('w:lvl')) if int(n.get(qn('w:ilvl'))) == level)
        pattern = definition.find(qn('w:lvlText')).get(qn('w:val'))
        label = re.sub(r'%([1-9])', lambda match: str(values[int(match[1]) - 1]), pattern)
        labels.append((paragraph.text, label))
        assert 0 <= level <= 8
    return document, labels


def test_full_host_paths_survive_sixteen_levels_and_returning_to_level_nine_in_main_and_volume(tmp_path):
    store = ArtifactStore(tmp_path)
    outline = deep_outline()
    expected = [(title, '.'.join(map(str, path))) for title, path in expected_paths(outline['chapters'])]
    result = export_authoring_document(store, outline, [])
    data = store.read_bytes(result['docx_ref'])
    document, labels = native_labels(data)
    assert labels == expected
    assert len(next(label for title, label in labels if title == '最深业务项').split('.')) == 16
    assert [p.text for p in document.paragraphs if p.style.name.startswith('Heading')] == [title for title, _ in expected]
    toc = [''.join(node.text or '' for node in link.iter(qn('w:t')))
           for link in document.element.iter(qn('w:hyperlink'))]
    assert toc == [label + ' ' + title for title, label in expected]
    with ZipFile(BytesIO(store.read_bytes(result['package_ref']))) as package:
        volumes = json.loads(package.read('manifest.json'))['volumes']
        assert native_labels(package.read(volumes[1]['package_path']))[1] == expected[1:]


def test_auxiliary_pending_sections_are_visible_but_never_body_navigation_headings(tmp_path):
    store = ArtifactStore(tmp_path)
    ref = {'item_ref': {'artifact_ref': store.put_json({'item_id': 'rule'}), 'item_id': 'rule'}}
    outline = {'chapters': [chapter('body', '正式正文')],
               'unplaced_targets': [target('unplaced')],
               'issues': [{'code': 'outline_directory_conflict', 'detail': '保留来源待核。', 'rule_refs': []}]}
    result = export_authoring_document(store, outline, [], requirements={'rules': [
        {'rule_ref': ref, 'rule': {'text': '未安排要求原文。', 'evidence': [{'quote': '未安排要求原文。'}]}}]})
    data = store.read_bytes(result['docx_ref'])
    document = Document(BytesIO(data))
    auxiliary = {'尚未归入响应表', '尚未安排的原响应', '适用性与来源待核'}
    assert auxiliary <= {p.text for p in document.paragraphs}
    assert all(p.style.name == 'Title' for p in document.paragraphs if p.text in auxiliary)
    assert [p.text for p in document.paragraphs if p.style.name.startswith('Heading')] == ['正式正文']
    assert [item['title'] for item in read_docx_outline(data)['chapters']] == ['正式正文']
    toc = [''.join(node.text or '' for node in link.iter(qn('w:t')))
           for link in document.element.iter(qn('w:hyperlink'))]
    assert toc == ['1 正式正文']


@pytest.mark.parametrize('seed', ['1', '2', '3'])
def test_orphan_units_keep_their_input_order_across_python_hash_seeds(tmp_path, seed):
    script = '''
from io import BytesIO
import json
import sys
from docx import Document
from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document
store = ArtifactStore(sys.argv[1])
units = [{'task_id': 'task-' + name, 'chapter_id': name, 'target_ids': [], 'status': 'generated',
          'blocks': [{'kind': 'paragraph', 'text': 'ORPHAN_' + name}], 'cell_fills': [], 'attachments': [], 'gaps': []}
         for name in ('old-a', 'old-b', 'old-c')]
outline = {'chapters': [{'chapter_id': 'body', 'title': '正文', 'targets': [], 'children': []}]}
result = export_authoring_document(store, outline, units)
document = Document(BytesIO(store.read_bytes(result['docx_ref'])))
print(json.dumps([p.text for p in document.paragraphs if p.text.startswith('ORPHAN_')]))
'''
    environment = {**os.environ, 'PYTHONHASHSEED': seed, 'PYTHONDONTWRITEBYTECODE': '1',
                   'PYTHONPATH': str(Path(__file__).resolve().parents[3])}
    result = subprocess.run([sys.executable, '-c', script, str(tmp_path / seed)], env=environment,
                            capture_output=True, text=True, check=True)
    assert json.loads(result.stdout) == ['ORPHAN_old-a', 'ORPHAN_old-b', 'ORPHAN_old-c']
