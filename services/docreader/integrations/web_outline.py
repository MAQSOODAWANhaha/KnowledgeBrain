"""Read immutable outline history and server-owned response identities."""
from __future__ import annotations

import asyncio

from docreader.integrations.artifacts import ArtifactIntegrityError
from docreader.tender.evidence.references import EvidenceRepository, selector
from docreader.tender.status import DocumentRole


def _targets(chapters):
    for chapter in chapters:
        yield from chapter['targets']
        yield from _targets(chapter['children'])


def _owned_targets(outline):
    yield from outline.get('unplaced_targets', [])
    yield from _targets(outline['chapters'])


async def outline_history(bridge, project_id):
    async with bridge.store.transaction() as conn:
        rows = await (await conn.execute(
            'SELECT revision_id,revision,parent_id,created_at FROM tender_outline_revisions '
            'WHERE project_id=%s ORDER BY revision DESC', (project_id,))).fetchall()
    return {'revisions': [{**{key: str(row[key]) if row[key] is not None else None
                              for key in ('revision_id', 'parent_id', 'created_at')},
                           'revision': row['revision']} for row in rows]}


async def response_catalog(bridge, project_id, base, *, revision=None, execution_id=None):
    """Only reuse definitions from the selected source and its exact input snapshot."""
    catalog = {}
    if execution_id is not None:
        _, original = await bridge.execution_outline(project_id, execution_id)
        if original['authoring_input'] == base['authoring_input']:
            catalog.update((target['target_id'], target) for target in _owned_targets(original))
    if revision is not None:
        async with bridge.store.transaction() as conn:
            rows = await (await conn.execute(
                'WITH RECURSIVE lineage AS ('
                'SELECT revision_id,parent_id,revision,execution_id,outline_ref FROM tender_outline_revisions '
                'WHERE project_id=%s AND revision_id=%s UNION ALL '
                'SELECT r.revision_id,r.parent_id,r.revision,r.execution_id,r.outline_ref FROM tender_outline_revisions r '
                'JOIN lineage l ON r.revision_id=l.parent_id WHERE r.project_id=%s) '
                'SELECT outline_ref,execution_id FROM lineage ORDER BY revision',
                (project_id, revision['revision_id'], project_id))).fetchall()
        for row in rows:
            if execution_id is not None and row['execution_id'] != execution_id:
                continue
            outline = await asyncio.to_thread(bridge.artifacts.read_json, row['outline_ref'])
            if outline['authoring_input'] == base['authoring_input']:
                catalog.update((target['target_id'], target) for target in _owned_targets(outline))
    catalog.update((target['target_id'], target) for target in _owned_targets(base))
    return list(catalog.values())


async def outline_detail(bridge, project_id, revision_id):
    row = await bridge.outline_revision(project_id, revision_id)
    draft = await bridge.outline_draft(row)
    catalog = await response_catalog(bridge, project_id, draft['outline'], revision=row,
                                     execution_id=row['execution_id'])
    return {'draft': draft, 'available_targets': catalog}


def project_context_sources(artifacts, outline) -> dict:
    """Project selectable main-document ranges for authoring project context."""
    frozen_ref = ((outline or {}).get('authoring_input') or {}).get('frozen_ref')
    if not isinstance(frozen_ref, dict):
        return {
            'available': False,
            'reason': '当前大纲缺少冻结主件来源，暂不可选择项目背景。',
            'frozen_sha256': None,
            'options': [],
        }
    try:
        frozen = artifacts.read_json(frozen_ref)
    except (OSError, ValueError, KeyError, ArtifactIntegrityError):
        return {
            'available': False,
            'reason': '当前大纲缺少冻结主件来源，暂不可选择项目背景。',
            'frozen_sha256': None,
            'options': [],
        }
    frozen_sha = frozen_ref.get('sha256')
    docs = frozen.get('documents') or []
    if any(doc.get('role') in (DocumentRole.AMENDMENT, DocumentRole.CLARIFICATION, 'amendment', 'clarification')
           for doc in docs):
        return {
            'available': False,
            'reason': '主件有补遗或澄清，请核对适用范围后再绑定方案背景。',
            'frozen_sha256': frozen_sha,
            'options': [],
        }
    repository = EvidenceRepository(artifacts)
    options = []
    for doc in docs:
        if doc.get('role') not in (DocumentRole.MAIN, 'main') or not isinstance(doc.get('source_index_ref'), dict):
            continue
        index_ref = doc['source_index_ref']
        try:
            index = repository.source_index(index_ref)
        except (OSError, ValueError, KeyError, ArtifactIntegrityError):
            continue
        for row in index.ranges:
            text = row.get('text') or ''
            if not text.strip():
                continue
            sel = selector(row)
            if sel['start'] >= sel['end']:
                continue
            locator = row.get('physical_locator') or row.get('locator') or {}
            page_ordinal = locator.get('page_ordinal')
            page = page_ordinal + 1 if type(page_ordinal) is int else None
            heading_path = row.get('heading_path') or locator.get('heading_path') or None
            options.append({
                'option_id': f"{index_ref['sha256'][:12]}:{row['range_id']}",
                'range_id': row['range_id'],
                'document_id': doc.get('document_id', ''),
                'page': page,
                'heading_path': heading_path,
                'preview': ' '.join(text.split())[:160],
                'excerpt': {
                    'source_ref': {'source_index_ref': index_ref, 'unit_id': row['unit_id']},
                    'selectors': [sel],
                },
            })
    return {
        'available': bool(options),
        'reason': None if options else '当前冻结主件没有可选文本范围。',
        'frozen_sha256': frozen_sha,
        'options': options,
    }
