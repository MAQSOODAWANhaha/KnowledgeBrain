"""Main Word, independent volumes and ZIP publish the same finalized bytes."""

from hashlib import sha256
from io import BytesIO
import json
from zipfile import ZipFile

from docx import Document
import pytest

from docreader.integrations.artifacts import ArtifactStore
from docreader.tender.authoring_export import export_authoring_document
from docreader.tender import toc_layout
from test_toc_pagination import chapter


def source_outline():
    def volume(identity):
        target = {'target_id': identity + ':target', 'kind': 'narrative',
            'purpose': '公章：________\n签字：________', 'rule_refs': [], 'source_refs': [],
            'manual_fill': True, 'support_origins': ['tender']}
        return {**chapter(identity, identity, children=[chapter(identity + '-child', '附证资料', targets=[target])]),
                'is_volume': True}
    return {'chapters': [volume('商务文件'), volume('技术文件')], 'issues': []}


@pytest.mark.parametrize('fail', [False, True])
def test_export_finalizes_each_volume_and_keeps_zip_refs_and_failure_candidates(tmp_path, monkeypatch, fail):
    artifacts, calls = ArtifactStore(tmp_path / 'artifacts'), []

    def render(blob):
        calls.append(sha256(blob).hexdigest())
        return b'%PDF-fixture', 'fixture-engine'

    def finalize(blob, callback):
        callback(blob)
        if fail:
            raise toc_layout.LayoutError('TOC_RENDER_FAILED', '目录转换失败。', candidate_docx=blob,
                                         receipt={'render_count': 1})
        result = BytesIO()
        with ZipFile(BytesIO(blob)) as source, ZipFile(result, 'w') as output:
            for info in source.infolist():
                value = source.read(info.filename)
                if info.filename == 'word/document.xml':
                    value = value.replace('待计算'.encode(), b'2')
                output.writestr(info, value)
        final = result.getvalue()
        return final, {'contract': 'toc_layout_v1', 'status': 'fixed', 'render_count': 1,
            'source_docx_sha256': sha256(blob).hexdigest(), 'docx_sha256': sha256(final).hexdigest(),
            'pdf_sha256': sha256(b'%PDF-fixture').hexdigest()}

    monkeypatch.setattr(toc_layout, 'finalize_toc', finalize)
    exported = export_authoring_document(artifacts, source_outline(), [], render_pdf=render)
    assert len(calls) == 3
    report = artifacts.read_json(exported['layout_ref'])
    assert len(report['documents']) == 3
    assert report['status'] == ('unverified' if fail else 'fixed')
    with ZipFile(BytesIO(artifacts.read_bytes(exported['package_ref']))) as package:
        manifest = json.loads(package.read('manifest.json'))
        assert manifest['layout']['artifact_ref'] == exported['layout_ref']
        assert json.loads(package.read(manifest['layout']['package_path'])) == report
        entries = [manifest['document'], *manifest['volumes']]
        assert len(entries) == 3
        assert exported['docx_ref'] == entries[0]['artifact_ref']
        for entry, checked in zip(entries, report['documents']):
            blob = artifacts.read_bytes(entry['artifact_ref'])
            assert package.read(entry['package_path']) == blob
            assert checked['artifact_ref'] == entry['artifact_ref']
            assert checked['source_ref']['sha256'] in calls
            document = Document(BytesIO(blob))
            assert '公章：________' in '\n'.join(p.text for p in document.paragraphs)
            assert '签字：________' in '\n'.join(p.text for p in document.paragraphs)
            xml = ZipFile(BytesIO(blob)).read('word/document.xml')
            assert ('待计算'.encode() in xml) is fail
            if fail:
                assert checked['status'] == 'unverified' and checked['error_code'] == 'TOC_RENDER_FAILED'
                assert artifacts.read_bytes(checked['candidate_ref']) == blob
    assert any(g['code'] == 'export_layout_unverified' for g in exported['gaps']) is fail
