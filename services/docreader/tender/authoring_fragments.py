"""Finite authoring splits at complete business/source boundaries, never characters."""
from __future__ import annotations

from copy import deepcopy
import json


def _key(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(',', ':'))


def _unique(values):
    return list({_key(value): value for value in values}.values())


def _source_inventory(original, selected, all_selected, common=()):
    selected_refs = _unique(part['source_ref'] for part in selected)
    selected_keys = {_key(ref) for ref in selected_refs}
    all_keys = {_key(part['source_ref']) for part in all_selected}
    common_keys = {_key(part['source_ref']) for part in common}
    omitted_keys = all_keys - selected_keys - common_keys
    result = [ref for ref in original if _key(ref) not in omitted_keys]
    return _unique([*result, *selected_refs, *(part['source_ref'] for part in common)])


def split_writing_task(task):
    """Return two smaller single-target tasks, or [] for an indivisible scope.

    The caller measures each complete request and may repeat this finite split on
    a smaller result. Task and target identities are unchanged here; the caller
    derives its work-unit suffix from the resulting scope, never a runtime ordinal.
    Native templates remain intact until a safe grid-specific split is available.
    """
    if len(task['targets']) != 1:
        return []
    target = task['targets'][0]
    rows = (target.get('response_table') or {}).get('rows')
    if target.get('manual_fill') or target.get('template_refs') or (
            target['kind'] != 'narrative' and rows is None):
        return []
    rules = _unique(target['rule_refs'])
    excerpts = target.get('source_excerpts', [])
    if rows is not None:
        groups = rows
    elif len(rules) > 1:
        groups = rules
    elif not rules:
        groups = excerpts
    else:
        return []
    if len(groups) < 2:
        return []
    midpoint = len(groups) // 2
    result = []
    for selected in (groups[:midpoint], groups[midpoint:]):
        part = deepcopy(task)
        narrowed = part['targets'][0]
        if rows is not None:
            narrowed['response_table']['rows'] = deepcopy(selected)
            row_rule_keys = {_key(ref) for row in rows for ref in row['rule_refs']}
            common_rules = [ref for ref in rules if _key(ref) not in row_rule_keys]
            narrowed['rule_refs'] = deepcopy(_unique([
                *common_rules, *(ref for row in selected for ref in row['rule_refs'])]))
            all_excerpts = [ref for row in rows for ref in row['source_excerpts']]
            selected_excerpts = [ref for row in selected for ref in row['source_excerpts']]
            narrowed['source_refs'] = deepcopy(_source_inventory(
                target['source_refs'], selected_excerpts, all_excerpts,
                [*excerpts, *target['response_table'].get('context_source_excerpts', [])]))
        elif rules:
            # Explicit source selections are common context. A complete rule is
            # loaded with all its own evidence/conditions by the normal writer.
            narrowed['rule_refs'] = deepcopy(selected)
        else:
            narrowed['source_excerpts'] = deepcopy(selected)
            narrowed['source_refs'] = deepcopy(_source_inventory(
                target['source_refs'], selected, excerpts))
        result.append(part)
    return result
