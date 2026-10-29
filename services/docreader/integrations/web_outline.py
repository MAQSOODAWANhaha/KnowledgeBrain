"""Read immutable outline history and server-owned response identities."""
from __future__ import annotations

import asyncio


def _targets(chapters):
    for chapter in chapters:
        yield from chapter['targets']
        yield from _targets(chapter['children'])


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
            catalog.update((target['target_id'], target) for target in _targets(original['chapters']))
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
                catalog.update((target['target_id'], target) for target in _targets(outline['chapters']))
    catalog.update((target['target_id'], target) for target in _targets(base['chapters']))
    return list(catalog.values())


async def outline_detail(bridge, project_id, revision_id):
    row = await bridge.outline_revision(project_id, revision_id)
    draft = await bridge.outline_draft(row)
    catalog = await response_catalog(bridge, project_id, draft['outline'], revision=row,
                                     execution_id=row['execution_id'])
    return {'draft': draft, 'available_targets': catalog}
