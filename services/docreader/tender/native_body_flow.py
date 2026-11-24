"""Resolve final source fields with bounded choices; preserve pending source domains."""
from copy import deepcopy
import json

from docreader.llm.request import build_request
from docreader.tender.native_body_selection import CONTRACT, plan_native_body_packs
from docreader.tender.source_body import bind_source_body
from docreader.tender.status import AuthoringTask
from docreader.tender.prompts import execution_prompt


class SourceBodyResolutionPending(RuntimeError):
    """Source choices or joins still require review; automatic publication stops."""


def _targets(outline):
    def walk(chapters):
        for chapter in chapters:
            yield from chapter.get('targets', [])
            yield from walk(chapter.get('children', []))
    yield from walk(outline.get('chapters', []))
    yield from outline.get('unplaced_targets', [])


def selection_request(budget, scope, view):
    """The measured and sent body includes the complete finite response schema."""
    prompt = execution_prompt(budget.prompts, 'directory_native_body')
    prompt += '\n\n返回JSON须符合以下完整JSON Schema：\n' + json.dumps(
        view.response_schema(), ensure_ascii=False, sort_keys=True, separators=(',', ':'))
    return build_request(budget.model, prompt, scope.request_payload(view),
                         reasoning_effort=budget.reasoning_effort)


async def resolve_native_bodies(artifacts, snapshot, outline, scope, budget, execute):
    """Reuse exact accepted work through the executor; no unbounded or hidden retries."""
    result = deepcopy(outline)
    targets = {t['target_id']: t for t in _targets(result)}
    for target_id, exact in getattr(scope, 'exact_bindings', {}).items():
        target = targets[target_id]
        target['source_context_excerpts'] = deepcopy(exact.get('context_source_excerpts', []))
        bound = bind_source_body(artifacts, snapshot, target, exact['parts'])
        target.clear()
        target.update(bound)
    planned = plan_native_body_packs(scope.selection_view(), budget,
        lambda view: selection_request(budget, scope, view), task=AuthoringTask.DIRECTORY_PARAMETER)
    records, pending = [], deepcopy(scope.pending)
    pending.extend(deepcopy(planned['pending_units']))
    from docreader.tender.authoring_pipeline import _plan

    await _plan(execute, AuthoringTask.DIRECTORY_PARAMETER, len(planned['packs']), 'native-bodies')
    for pack in planned['packs']:
        view, request = pack['view'], pack['request']
        # DIRECTORY_PARAMETER is an existing task; the dedicated operation,
        # prompt and full schema give R its own exact digest and durable budget.
        accepted = await execute(AuthoringTask.DIRECTORY_PARAMETER, request, view.accept)
        pending.extend(accepted['pending'])
        for unit in accepted['units']:
            binding = scope.bindings[unit['unit_id']]
            target = targets[binding['target_id']]
            record = {'unit_id': unit['unit_id'], 'target_id': target['target_id'],
                      'chapter_id': binding['chapter_id'], 'state': 'unresolved',
                      'enterprise_confirmed': False}
            record['selected_sources'] = [
                {'cell_id': piece['cell_id'], 'start': piece['start'], 'end': piece['end'],
                 'source_join': piece['source_join']} for island in unit['islands'] for piece in island['body']]
            if any(island['state'] != 'selected' for island in unit['islands']):
                records.append(record)
                continue
            if binding.get('layout_pending_rows'):
                record['state'] = 'selected_pending_layout'
                records.append(record)
                # Exact native choices cannot silently discard unregistered
                # continuation lines or decide their join with native text.
                pending.append({'target_id': target['target_id'], 'reason': 'mixed_source_join_unresolved',
                    'outlet': 'manual_source_review', 'enterprise_confirmed': False})
                continue
            parts = [{'source_excerpt': scope.cell_excerpt(piece['cell_id'], piece['start'], piece['end']),
                      'join': piece['source_join']} for island in unit['islands'] for piece in island['body']]
            context = [scope.cell_excerpt(piece['cell_id'], piece['start'], piece['end'])
                       for piece in unit['context']]
            target['source_context_excerpts'] = context
            bound = bind_source_body(artifacts, snapshot, target, parts)
            target.clear()
            target.update(bound)
            record.update(state='bound', source_body_ref=target['source_body_ref'])
            records.append(record)
    return result, {'contract': CONTRACT, 'scope_binding': scope.binding,
                    'scope_records': deepcopy(scope.records), 'records': records,
                    'source_body_bindings': [{'target_id': target['target_id'], 'source_body_ref': target['source_body_ref']}
                                             for target in targets.values() if target.get('source_body_ref')],
                    'pending': pending, 'complete': not pending,
                    'planned_requests': len(planned['packs'])}
