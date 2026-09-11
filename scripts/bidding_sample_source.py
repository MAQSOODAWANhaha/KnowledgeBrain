#!/usr/bin/env python3
"""Freeze a local acceptance source using the existing Python docreader service.

No chapter inference, document association or bidder-data defaults. Run with the
service virtualenv and PYTHONPATH=services. All budgets come from the run file.
"""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--limits', type=Path, required=True)
    args = parser.parse_args()
    from docreader.main import DocReaderServicer
    from docreader.proto.docreader_pb2 import ReadConfig, ReadRequest, SourceViewRequest
    from docreader.parser.source_view import source_view

    raw = args.source.read_bytes()
    sha = hashlib.sha256(raw).hexdigest()
    limits = json.loads(args.limits.read_text())['extraction']
    out = args.output
    out.mkdir(parents=True, exist_ok=True)
    if (out / 'frozen-input.json').exists():
        raise SystemExit('frozen input exists; reuse it or choose a new output directory')
    parsed, _ = DocReaderServicer()._parse_request(ReadRequest(
        file_content=raw, file_name=args.source.name, file_type=args.source.suffix.lstrip('.'),
        config=ReadConfig(parser_engine='builtin')))
    if not parsed.is_valid() or parsed.metadata.get('table_extraction_error'):
        raise SystemExit('shared parser did not produce a valid complete result')
    identity = lambda key: str(uuid.uuid5(uuid.NAMESPACE_URL, f'{sha}/{key}'))
    document = identity('document')
    sources, forms = [], []
    for unit in parsed.structured_source_units:
        sid = identity(unit.key)
        locator = unit.locator.model_dump(mode='json')
        sources.append(dict(source_unit_revision_id=sid, document_id=document,
                            text=unit.text, locator=locator, ordinal=unit.ordinal))
        if unit.grid is not None:
            form_id = identity('form/' + unit.key)
            definition = dict(unit.grid.model_dump(mode='json', exclude_none=True),
                              schema_version=3, kind='grid',
                              form_definition_revision_id=form_id,
                              source_unit_revision_id=sid, title='source_unit:' + sid)
            forms.append(dict(form_definition_revision_id=form_id,
                              source_unit_revision_id=sid, definition=definition))
    frozen = dict(schema_version=1, project_id=identity('sample-project'),
                  document_set_id=identity('document-set'),
                  documents=[dict(document_id=document, file_name=args.source.name,
                                  sha256=sha, byte_length=len(raw), role='main_tender')],
                  document_relations=[], decisions=[], source_units=sources, structured_forms=forms)
    (out / 'parsed-source.json').write_text(parsed.model_dump_json(), encoding='utf-8')
    if args.source.suffix.lower() == '.pdf':
        views = out / 'source-views'
        views.mkdir(exist_ok=True)
        for page in range(parsed.metadata['page_count']):
            result = source_view(SourceViewRequest(file_content=raw, source_sha256=sha,
                media_type='application/pdf', page_ordinal=page,
                max_edge=limits['max_source_view_edge'], max_image_bytes=limits['max_source_view_bytes']))
            payload = dict(identity=dict(source_id='', original_sha256=sha,
                image_sha256=result.image_sha256, page_ordinal=page, width=result.width,
                height=result.height, renderer=result.renderer),
                jpeg_base64=base64.b64encode(result.image_data).decode('ascii'))
            (views / f'page-{page}.json').write_text(json.dumps(payload), encoding='utf-8')
    # Publish only after all original views exist. A failed freeze can be
    # retried; the runner never sees a supposedly complete partial collection.
    (out / 'frozen-input.json').write_text(json.dumps(frozen, ensure_ascii=False), encoding='utf-8')
    print(json.dumps(dict(source_sha256=sha, sources=len(sources), forms=len(forms))))


if __name__ == '__main__':
    main()
