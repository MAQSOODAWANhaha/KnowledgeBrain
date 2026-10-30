"""Bounded document authoring: semantics in model calls, references in the host.

The executor owns paid retries and receipts. Business gaps become located output;
cancellation, lease loss and storage errors retain their infrastructure semantics.
"""
from __future__ import annotations

import asyncio
from copy import deepcopy
import hashlib
from importlib.resources import files
import json

from pydantic import ValidationError

from docreader.llm.request import ModelBudgetError, ModelContractError, ModelWorkUnitExhausted, build_request
from docreader.llm.wire import WirePayload
from docreader.tender.authoring import (
    _OutlineContext, _check_reference_ids, load_outline_source, project_source_sections, validate_outline_references,
)
from docreader.tender.authoring_templates import (
    prepare_template_parts, load_template_part, template_part_key, ordered_template_part_ids, template_part_runs, expand_template_spans,
)
from docreader.tender.authoring_excerpts import (
    load_source_excerpt, source_excerpt_key, whole_source_excerpts, response_row_id,
)
from docreader.tender.evidence.references import EvidenceRepository
from docreader.tender.schema import (
    AUTHORING_GROUNDING_POLICY,
    AuthoringBasis, AuthoringBasisContent, AuthoringBasisResponse, AuthoringDocument, AuthoringInput,
    AuthoringOutline, AuthoringReviewResponse, AuthoringUsage, KnowledgeMaterial,
    OutlineAssemblyResponse, OutlineResponse, SourceUnitRef, WritingResponse, WritingTask, WritingUnit,
)


_LOCAL_FAILURES = (ModelWorkUnitExhausted, ModelBudgetError)
_OUTLINE_CORRECTION_TOKENS = 2048


def _json(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':'))


def _parse(schema, value):
    try:
        return schema.model_validate(value).model_dump()
    except ValidationError as error:
        raise ModelContractError([{'path': list(e['loc']), 'code': e['type'], 'detail': e['msg']}
                                  for e in error.errors(include_input=False)]) from None


def _prompt(budget, task):
    return budget.prompts.get(task) or files('docreader.tender.prompts').joinpath(task + '.md').read_text('utf-8')


def _request(budget, task, payload):
    prompt_name = 'outline_assemble' if task == 'outline' and payload.get('operation') == 'assemble' else task
    return build_request(budget.model, _prompt(budget, prompt_name), payload, task=task)


def _fits(budget, task, payload, *, headroom_tokens=0):
    try:
        measured = budget.require_request(_request(budget, task, payload), task)
    except ModelBudgetError:
        return False
    return measured['input_tokens'] + headroom_tokens <= measured['effective_input_budget_tokens']


async def _ask(execute, budget, task, payload, validate):
    request = _request(budget, task, payload)
    hit, value = await _replay(execute, task, request, validate)
    if hit:
        return value
    if (task == 'outline' and payload.get('operation') == 'assemble'
            and payload.get('assembly_view') != 'compact_v1'):
        if not await asyncio.to_thread(_fits, budget, task, payload):
            from docreader.tender.authoring_view import compact_assembly_payload
            compact = await asyncio.to_thread(compact_assembly_payload, payload)
            request = _request(budget, task, compact)
            hit, value = await _replay(execute, task, request, validate)
            if hit:
                return value
    budget.admit_request(request, task)
    return await execute(task, request, validate)


async def _replay(execute, task, request, validate):
    replay = getattr(execute, 'replay', None)
    return await replay(task, request, validate) if replay is not None else (False, None)


def _batches(records, budget, task, payload, *, headroom_tokens=0):
    """Greedy packing at record boundaries; never truncate an indivisible record."""
    batch = []
    for record in records:
        if batch and not _fits(budget, task, payload([*batch, record]), headroom_tokens=headroom_tokens):
            yield batch
            batch = []
        batch.append(record)
    if batch:
        yield batch


def _gap(code, detail, task=None):
    result = {'code': code, 'detail': detail}
    if task:
        result.update(task_id=task['task_id'], chapter_id=task['chapter_id'])
        if len(task['targets']) == 1:
            result['target_id'] = task['targets'][0]['target_id']
    return result


def prepare_materials(artifacts, materials):
    result, issues, seen = [], [], {}
    for raw in materials:
        material = KnowledgeMaterial.model_validate(raw).model_dump()
        if material['material_id'] in seen:
            if material != seen[material['material_id']]:
                raise ValueError('material_id identifies different material content')
            continue
        seen[material['material_id']] = deepcopy(material)
        # Text snapshots and original files have deliberately different roles.
        material['content_ref'] = artifacts.put_bytes(material['text'].encode(), 'text/plain')
        if material['original_ref'] is not None:
            try:
                artifacts.path(material['original_ref'])
            except (OSError, ValueError):
                material['original_ref'] = None
                issues.append(_gap('material_original_unavailable', f"资料原件不可读：{material['title']}。"))
        if material['attachment'] and material['original_ref'] is None:
            material['attachment'] = False
            issues.append(_gap('material_original_missing', f"资料仅有文字片段，不能作为证明原件：{material['title']}。"))
        if material['attachment'] and not material['text'].strip():
            issues.append(_gap('material_evidence_unreadable', f"证明没有可判断的文字依据，保留待核位置：{material['title']}。"))
        result.append(material)
    return result, issues


def _material_views(materials, targets=()):
    result = []
    for ordinal, material in enumerate(materials, 1):
        view = {key: material[key] for key in ('title', 'text', 'usage', 'attachment')}
        view.update(support_id=f'k{ordinal}', origin='knowledge')
        view['locator'] = {key: material['locator'][key] for key in (
            'product_id', 'version_id', 'version_label', 'document_id', 'is_current') if key in material['locator']}
        if targets:
            view['target_ids'] = [f't{i + 1}' for i, target in enumerate(targets)
                                  if _material_applies(material, target)]
        result.append(view)
    return result


def _material_applies(material, target):
    scope = material.get('locator', {}).get('target_ids')
    return (not target.get('manual_fill', False)
            and 'knowledge' in target.get('support_origins', ['knowledge'])
            and (not scope or target['target_id'] in scope)
            and bool(material.get('text', '').strip()))


def _knowledge_queries(outline, tasks, gaps):
    queries = []
    for task in tasks:
        for target in task['targets']:
            if target.get('manual_fill', False) or 'knowledge' not in target.get('support_origins', ['knowledge']):
                continue
            rules = {_json(ref) for ref in target['rule_refs']}
            sources = {_json(ref) for ref in target['source_refs']}
            ranges = {selected['range_id'] for ref in [*target.get('source_excerpts', []),
                *(target.get('response_table') or {}).get('context_source_excerpts', []),
                *(ref for row in (target.get('response_table') or {}).get('rows', [])
                  for ref in row['source_excerpts'])] for selected in ref['selectors']}
            selected = [intent for intent in outline.get('search_intents', [])
                        if rules.intersection(_json(ref) for ref in intent['rule_refs'])
                        or sources.intersection(_json(ref) for ref in intent['source_refs'])
                        or ranges.intersection(intent.get('range_ids', []))]
            if not selected:
                gaps.append({**_gap('retrieval_plan_missing', '本位置缺少基于实际原文的检索意图，使用已有资料并保留缺口。', task),
                             'target_id': target['target_id']})
            queries.extend({'target_id': target['target_id'], 'query': intent['query'],
                            'include_original': target['kind'] == 'attachment'} for intent in selected)
    return list({_json(query): query for query in queries}.values())


def _task_materials(materials, task):
    return [m for m in materials if any(_material_applies(m, t) for t in task['targets'])]


def _support_bindings(sources, materials):
    return {**{identity: {'origin': 'tender', 'source_ref': ref.get('source_ref', ref),
                         **({'source_excerpt': ref} if 'selectors' in ref else {})}
               for identity, ref in sources.items()},
            **{f'k{i}': {'origin': 'knowledge', 'material_id': m['material_id']}
               for i, m in enumerate(materials, 1)}}


def _has_allowed_support(payload):
    targets = {t['target_id']: t for t in payload['targets'] if not t.get('manual_fill', False)}
    response_targets = {row['target_id'] for row in payload.get('response_rows', [])}
    return any((s.get('text', '').strip() or s.get('grid')) and
               any(identity in targets and s['origin'] in targets[identity].get('support_origins', ['knowledge'])
                   and (identity not in response_targets or s['origin'] == 'knowledge')
                   for identity in s.get('target_ids', [])) for s in payload['supports'])


def _source_records(artifacts, context):
    """Read every frozen range once for organization, including non-item background."""
    records, included_rules = [], set()
    repository = getattr(context, 'repository', None) or EvidenceRepository(artifacts)
    rule_views = context.payload['rules']
    index_by_range = {}
    for rule in rule_views:
        semantic = rule.get('item', rule.get('effective_rule', {}))
        for evidence in semantic.get('evidence', []):
            for ref in context.sources.get(evidence['range_id'], []):
                index_by_range.setdefault((ref['source_index_ref']['sha256'], ref['unit_id']), []).append(rule)
    if context.input['frozen_ref']:
        try:
            frozen = artifacts.read_json(context.input['frozen_ref'])
        except (OSError, ValueError):
            frozen = {}
        for document in frozen.get('documents', []):
            try:
                index = repository.source_index(document['source_index_ref'])
            except (OSError, ValueError):
                continue  # The input/context already records this unavailable source.
            for source in index.ranges:
                related = []
                for ref in context.sources.get(source['range_id'], []):
                    related.extend(index_by_range.get((ref['source_index_ref']['sha256'], ref['unit_id']), []))
                related = list({r['rule_id']: r for r in related if r['rule_id'] not in included_rules}.values())
                included_rules.update(r['rule_id'] for r in related)
                records.append({'source': {'range_id': source['range_id'], 'text': source['text'],
                    'heading_path': source.get('heading_path', ''), 'kind': source['kind'],
                    'role': document['role']}, 'rules': related})
    records.extend({'rules': [r]} for r in rule_views if r['rule_id'] not in included_rules)
    return records


def _analysis_context(context, rules, source_ids=(), explicit_issue_ids=()):
    """Route existing semantic observations by identity, without reinterpreting them."""
    item_ids, ranges = set(), set(source_ids)
    for rule in rules:
        semantic = rule.get('item', rule.get('effective_rule', {}))
        identity = rule.get('item_id') or semantic.get('item_id')
        if identity:
            item_ids.add(identity)
        ranges.update(e['range_id'] for e in semantic.get('evidence', []))
    # A table unit may contain many independent source rows. Only a fragment's
    # canonical range, not every row in its native table, shares its issue scope.
    roots = getattr(context, 'range_roots', {})
    original_ranges = set(ranges)
    ranges.update(roots.get(identity, identity) for identity in original_ranges)
    ranges.update(identity for identity, root in roots.items() if root in original_ranges)
    relations = [r for r in context.payload.get('relations', [])
                 if r.get('from_item_id') in item_ids or r.get('to_item_id') in item_ids
                 or any(e['range_id'] in ranges for e in r.get('evidence', []))]
    issues = [i for i in context.payload.get('issues', [])
              if i.get('range_id') in ranges or set(i.get('item_ids') or []) & item_ids
              or i.get('issue_id') in explicit_issue_ids]
    issue_ids = {i.get('issue_id') for i in issues}
    resolutions = context.payload.get('issue_resolutions') or {}
    resolutions = resolutions.get('resolutions', []) if isinstance(resolutions, dict) else resolutions
    related_ids = {r[key] for r in relations for key in ('from_item_id', 'to_item_id')} - item_ids
    related_rules = [{k: v for k, v in r.items() if k != 'rule_id'} for r in context.payload['rules']
                     if (r.get('item_id') or r.get('item', {}).get('item_id')) in related_ids]
    return {'relations': relations, 'related_rules': related_rules, 'analysis_issues': issues,
            'unverified_relation_ids': [r['relation_id'] for r in relations
                                       if r['relation_id'] in context.payload.get('unverified_relation_ids', [])],
            'issue_resolutions': [r for r in resolutions if r['issue_id'] in issue_ids]}


def _record_sources(record):
    return record.get('sources', [record['source']] if 'source' in record else [])


def _outline_payload(context, records, template_parts=()):
    rules = list({r['rule_id']: r for entry in records for r in entry.get('rules', [])}.values())
    sources = [source for entry in records for source in _record_sources(entry)]
    visible = {source['range_id'] for source in sources}
    sections, projected = project_source_sections(sources)
    return {'schema_version': 2, 'grounding_policy': AUTHORING_GROUNDING_POLICY, 'rules': rules,
            'sources': projected, 'sections': sections,
            'template_parts': [part for part in template_parts if set(part['range_ids']) <= visible],
            'rule_origin': context.payload.get('rule_origin', 'extraction'),
            'input_issues': context.input['issues'],
            'scope_decisions': context.payload.get('scope_decisions', []),
            **_analysis_context(context, rules, [s['range_id'] for s in sources])}


def _constraint_context(context, refs, submission_refs=(), *, source_bindings=None, excerpt_bindings=None):
    """Only model-selected submission rules and their real source evidence."""
    views = {rule['rule_id']: rule for rule in context.payload['rules']}
    by_ref = {_json(ref): views[identity] for identity, ref in context.rules.items()}
    rules, sources, used = [], [], {}
    def source(text, identity, references=(), excerpts=()):
        if not text.strip():
            return None
        key = (identity, text)
        if key not in used:
            used[key] = f's{len(sources) + 1}'
            sources.append({'source_id': used[key], 'text': text})
            if source_bindings is not None:
                if references:
                    source_bindings[used[key]] = list({_json(ref): deepcopy(ref) for ref in references}.values())
                else:
                    sources[-1]['bindable'] = False
            if excerpt_bindings is not None:
                excerpt_bindings[used[key]] = list(excerpts)
        return used[key]
    for ref in refs:
        rule = by_ref.get(_json(ref))
        if rule is None:
            continue
        semantic = rule.get('item', rule.get('effective_rule', {}))
        supports = [source(e.get('quote', ''), e.get('range_id'), context.sources.get(e.get('range_id'), []),
                           getattr(context, 'excerpts', {}).get(e.get('range_id'), []))
                    for e in semantic.get('evidence', [])]
        rules.append({'rule_id': f'e{len(rules) + 1}',
                      **{field: semantic[field] for field in ('text', 'kind', 'subject', 'scope',
                          'conditions', 'exceptions') if field in semantic},
                      'source_ids': [identity for identity in supports if identity]})
    artifacts = getattr(context, 'artifacts', None)
    repository = getattr(context, 'repository', None)
    source_options = {'repository': repository} if repository is not None else {}
    if artifacts is not None:
        for ref in submission_refs:
            try:
                loaded = load_outline_source(artifacts, ref, **source_options)
                unit = loaded['unit']
                text = unit.get('text', '')
                if unit.get('grid'):
                    text = _json(unit['grid'])
                excerpts = whole_source_excerpts(artifacts, ref, **source_options) if excerpt_bindings is not None else []
                source(text, _json(ref), [ref], excerpts)
            except (OSError, ValueError, KeyError):
                continue
    return {'submission_rules': rules, 'submission_sources': sources}


def _flatten(chapters):
    for chapter in chapters:
        yield chapter
        yield from _flatten(chapter['children'])


def _renumber(chapters):
    def walk(entries, parent=()):
        for ordinal, chapter in enumerate(entries, 1):
            path = (*parent, ordinal)
            chapter['chapter_id'] = 'chapter-' + '.'.join(map(str, path))
            for i, target in enumerate(chapter['targets'], 1):
                target['target_id'] = f"{chapter['chapter_id']}:target-{i}"
                for row in (target.get('response_table') or {}).get('rows', []):
                    row['row_id'] = response_row_id(target['target_id'], row['source_excerpts'], row['rule_refs'])
            walk(chapter['children'], path)
    result = deepcopy(chapters)
    walk(result)
    return result


async def _organize(artifacts, context, budget, execute, gaps, *, submission_sources=(), structure_results=None):
    def plan_sources():
        records, template_parts = prepare_template_parts(artifacts, context, _source_records(artifacts, context))
        # Original source content is visible in each local pass. No model coverage echo.
        context.read_units = set()
        def build(batch):
            return WirePayload('outline', _outline_payload(context, batch, template_parts)).payload
        expanded = []
        for row in records:
            if _record_sources(row) and row.get('rules') and not _fits(
                    budget, 'outline', build([row]), headroom_tokens=_OUTLINE_CORRECTION_TOKENS):
                expanded.append({key: value for key, value in row.items() if key != 'rules'} | {'rules': []})
                expanded.extend({'rules': [rule]} for rule in row['rules'])
            else:
                expanded.append(row)
        return template_parts, list(_batches(expanded, budget, 'outline', build,
            headroom_tokens=_OUTLINE_CORRECTION_TOKENS)) or [[]]
    template_parts, batches = await asyncio.to_thread(plan_sources)
    fragments = []
    for batch in batches:
        local_payload = _outline_payload(context, batch, template_parts)
        wire = WirePayload('outline', local_payload)
        try:
            local_rules = {r['rule_id'] for row in batch for r in row.get('rules', [])}
            read_sources = {source['range_id'] for row in batch for source in _record_sources(row)}
            local_sources = set(wire.mapping.get('r', {})) & context.sources.keys()
            local_parts = {part['part_id'] for part in local_payload['template_parts']}
            def validate_local(raw):
                value = _parse(OutlineResponse, wire.decode(raw))
                validate_outline_references(value, rule_ids=local_rules, source_ids=local_sources,
                    template_part_ids=local_parts, constraint_source_ids=read_sources)
                context.read_units.update((ref['source_index_ref']['sha256'], ref['unit_id'])
                                          for identity in read_sources for ref in context.sources.get(identity, []))
                return context.validate(value, template_part_ids=local_parts)
            fragments.append(await _ask(execute, budget, 'outline', wire.payload, validate_local))
        except _LOCAL_FAILURES as error:
            gaps.append(_gap('outline_batch_unavailable', f'部分原文组织未完成（{type(error).__name__}），保留待核对。'))
    if not fragments:
        return AuthoringOutline(schema_version=2, authoring_input=context.input, issues=[], chapters=[{
            'chapter_id': 'chapter-1', 'title': '投标文件待补内容', 'children': [],
            'targets': [{'target_id': 'chapter-1:target-1', 'kind': 'narrative',
                         'purpose': '根据现有资料起草；招标目录及要求待补，不声称已覆盖。',
                         'rule_refs': [], 'source_refs': []}]}]).model_dump()
    refs = list({_json(r): r for f in fragments for r in f.get('constraint_rule_refs', [])}.values())
    submission_refs = {_json(r): r for f in fragments for r in f.get('submission_refs', [])}
    available_sources = {_json(ref) for refs in context.sources.values() for ref in refs}
    for raw in submission_sources:
        ref = SourceUnitRef.model_validate(raw).model_dump()
        if _json(ref) in available_sources:
            submission_refs[_json(ref)] = ref
        else:
            gaps.append(_gap('outline_submission_source_unavailable',
                             '补充提交依据不属于当前可读的冻结来源，保留待核；其余内容继续。'))
    submission_refs = list(submission_refs.values())
    mapping, origins, entries, parts, part_map, part_ids = {}, {}, [], [], {}, {}
    candidate_roles, previous_positions, previous_selections = {}, {}, {}
    repository = getattr(context, 'repository', None) or EvidenceRepository(artifacts)
    source_bindings, submission_excerpt_bindings, submission_source_ids = {}, {}, set()
    excerpt_ids, excerpt_map, excerpt_views, assembly_rules = {}, {}, [], {}
    missing_rules = {}
    def candidates(chapters, parent=(), role='body'):
        for chapter in chapters:
            path = (*parent, chapter['title'])
            current = 'submission_note' if role == 'submission_note' else chapter.get('presentation_role', 'body')
            yield chapter, path, current
            yield from candidates(chapter['children'], path, current)
    def prepare_assembly():
        repository = getattr(context, 'repository', None)
        source_options = {'repository': repository} if repository is not None else {}
        def register_part(part):
            key = template_part_key(part)
            if key in part_ids:
                return part_ids[key]
            identity = f'p{len(parts) + 1}'
            part_ids[key] = identity
            part_map[identity] = part
            descriptor = {'part_id': identity}
            try:
                loaded = load_template_part(artifacts, part, **source_options)
                unit, grid = loaded['unit'], loaded.get('grid')
                descriptor.update(kind=unit['kind'], heading_path=unit.get(
                    'heading_path', unit.get('locator', {}).get('heading_path', '')))
                if grid:
                    descriptor['grid'] = grid
                else:
                    descriptor['text'] = loaded.get('text', '')
                    if unit['kind'] == 'image_region' and not descriptor['text'].strip():
                        descriptor['unavailable'] = True
            except (OSError, ValueError, KeyError):
                descriptor['unavailable'] = True
            parts.append(descriptor)
            return identity

        def register_excerpt(ref):
            key = source_excerpt_key(ref)
            if key not in excerpt_ids:
                identity = f'x{len(excerpt_map) + 1}'
                excerpt_ids[key] = identity
                excerpt_map[identity] = ref
                view = {'range_id': identity}
                try:
                    loaded = load_source_excerpt(artifacts, ref, **source_options)
                    view.update(text=loaded['text'], heading_path=loaded['unit'].get('heading_path', ''))
                except (OSError, ValueError, KeyError):
                    view['unavailable'] = True
                excerpt_views.append(view)
            return excerpt_ids[key]

        def register_rule(ref):
            for identity, value in assembly_rules.items():
                if value == ref:
                    return identity
            identity = f'a{len(assembly_rules) + 1}'
            assembly_rules[identity] = ref
            return identity
        for fragment_ordinal, fragment in enumerate(fragments):
            for chapter, path, role in candidates(fragment['chapters']):
                targets = []
                for target in chapter['targets']:
                    key = f't{len(mapping) + 1}'
                    mapping[key] = target
                    candidate_roles[key] = role
                    origins[key] = (fragment_ordinal, target['target_id'])
                    linked_parts = [register_part(part) for part in target.get('template_refs', [])]
                    descriptor = {'target_id': key, 'kind': target['kind'], 'purpose': target['purpose'],
                                    **({'subject': target['subject']} if target.get('subject') else {}),
                                    'manual_fill': target.get('manual_fill', False),
                                    'support_origins': target.get('support_origins', ['knowledge']),
                                    'template_part_ids': linked_parts}
                    if target.get('source_excerpts'):
                        descriptor['range_ids'] = [register_excerpt(ref) for ref in target['source_excerpts']]
                    if target.get('response_table'):
                        descriptor['response_table'] = {'context_range_ids': [register_excerpt(ref)
                            for ref in target['response_table'].get('context_source_excerpts', [])], 'rows': [
                            {'range_ids': [register_excerpt(ref) for ref in row['source_excerpts']],
                             'rule_ids': [register_rule(ref) for ref in row['rule_refs']]}
                            for row in target['response_table']['rows']]}
                    targets.append(descriptor)
                entries.append({'title': chapter['title'], 'title_path': list(path),
                                'is_volume': chapter.get('is_volume', False),
                                **({'presentation_role': role} if role != 'body' else {}), 'targets': targets})
        # A continuation already named by any candidate is reachable as a format.
        # Native membership authorizes availability, never automatic insertion.
        candidate_units = {_json(ref) for target in mapping.values() for ref in [
            *target['source_refs'], *(part['source_ref'] for part in target.get('template_refs', []))]}
        for part in getattr(context, 'template_parts', {}).values():
            if _json(part['source_ref']) in candidate_units:
                register_part(part)
        source_repository = repository or EvidenceRepository(artifacts)
        if parts:
            ordered = ordered_template_part_ids(part_map, source_repository)
            by_id = {part['part_id']: part for part in parts}
            parts[:] = [by_id[identity] for identity in ordered]
        covered = {_json(ref) for ref in refs}
        covered.update(_json(ref) for target in mapping.values() for ref in target['rule_refs'])
        views = {entry['rule_id']: entry for entry in context.payload['rules']}
        unassigned = []
        for identity, reference in context.rules.items():
            if _json(reference) in covered:
                continue
            alias = f'u{len(missing_rules) + 1}'
            missing_rules[alias] = reference
            view = views.get(identity, {})
            semantic = deepcopy(view.get('item', view.get('effective_rule', {})))
            semantic.pop('item_id', None)
            semantic['evidence'] = [{'quote': e['quote']} for e in semantic.get('evidence', [])]
            unassigned.append({'rule_id': alias, 'rule': semantic})
        sections, projected_parts = project_source_sections(parts)
        excerpt_sections, projected_excerpts = project_source_sections(excerpt_views)
        assembly_rules.update(missing_rules)
        submission = _constraint_context(context, refs, submission_refs, source_bindings=source_bindings,
                                         excerpt_bindings=submission_excerpt_bindings)
        submission_source_ids.update(source['source_id'] for source in submission['submission_sources'])
        return {'schema_version': 2, 'operation': 'assemble', 'chapters': entries,
                'template_sections': sections, 'template_parts': projected_parts,
                'template_runs': template_part_runs(part_map, source_repository,
                    all_parts=getattr(context, 'template_parts', None)),
                **({'source_sections': excerpt_sections, 'source_excerpts': projected_excerpts} if excerpt_views else {}),
                'unassigned_rules': unassigned,
                'findings': list({_json({'code': issue['code'], 'detail': issue['detail']}):
                    {'code': issue['code'], 'detail': issue['detail']}
                    for fragment in fragments for issue in fragment['issues']
                    if issue['code'] != 'outline_rule_unassigned'}.values()),
                **submission}
    payload = await asyncio.to_thread(prepare_assembly)
    try:
        def assemble(raw):
            parsed = _parse(OutlineAssemblyResponse, raw)
            used, used_rules, header_issues = set(), set(), []
            owners, links, selections = {}, [], []
            selected_parts = deepcopy(part_map)
            descriptors = {part['part_id']: part for part in parts}
            header_counts, invalid_headers = {}, set()
            for entry in parsed['table_headers']:
                identity, count = entry['part_id'], entry['header_row_count']
                grid = descriptors.get(identity, {}).get('grid') or {}
                previous = header_counts.get(identity, count)
                header_counts[identity] = count
                if (identity in invalid_headers or identity not in selected_parts
                        or (count and count >= grid.get('row_count', 0))
                        or not grid or previous != count
                        or any(cell['row'] < count < cell['row'] + cell.get('row_span', 1)
                               for cell in grid.get('cells', []))):
                    invalid_headers.add(identity)
                    header_issues.append({'code': 'outline_table_header_unverified',
                        'detail': '原表表头展示范围不可确认，保留原表并沿已有标记显示。', 'rule_refs': []})
                    if identity in selected_parts:
                        selected_parts[identity].pop('header_row_count', None)
                    continue
                selected_parts[identity]['header_row_count'] = count

            def rule_refs(identities, path):
                _check_reference_ids(identities, assembly_rules, path, 'Select only supplied rule references.')
                return [deepcopy(assembly_rules[i]) for i in dict.fromkeys(identities)]

            views = {entry['rule_id']: entry for entry in context.payload['rules']}
            by_ref = {_json(ref): views.get(identity, {}) for identity, ref in context.rules.items()}

            def template_override(path):
                value = raw
                for key in path:
                    value = value[key]
                return 'template_part_ids' in value or 'template_spans' in value

            def bind(chapters, path=('chapters',)):
                output = []
                for ordinal, c in enumerate(chapters):
                    chapter_path = (*path, ordinal)
                    targets = []
                    instances = []
                    for number, target in enumerate(c['targets']):
                        for field in ('target_ids', 'related_target_ids'):
                            _check_reference_ids(target[field], mapping, (*chapter_path, 'targets', number, field),
                                                 'Select supplied response-target references.')
                        subjects = list(dict.fromkeys(mapping[identity]['subject'] for identity in target['target_ids']
                            if identity in mapping and mapping[identity].get('subject')))
                        unknown_subjects = [identity for identity in dict.fromkeys(target['target_ids'])
                                            if not mapping[identity].get('subject')]
                        if len(subjects) > 1 or (subjects and unknown_subjects
                                and target['subject'] != subjects[0]):
                            for subject in subjects:
                                instances.append((number, {**target, 'subject': subject,
                                    'target_ids': [identity for identity in target['target_ids']
                                        if mapping.get(identity, {}).get('subject') == subject]}))
                            instances.extend((number, {**target, 'subject': None, 'target_ids': [identity]})
                                             for identity in unknown_subjects)
                            header_issues.append({'code': 'outline_subject_instances_retained',
                                'detail': '组装合并了不同填写主体，已保留来源明确的各份实例，归属待核。', 'rule_refs': []})
                        else:
                            instances.append((number, {**target, 'subject': target['subject'] or
                                (subjects[0] if subjects else None)}))
                    for number, target in instances:
                        target_path = (*chapter_path, 'targets', number)
                        for field in ('target_ids', 'related_target_ids'):
                            _check_reference_ids(target[field], mapping, (*target_path, field),
                                                 'Select supplied response-target references.')
                        selected = [mapping[key] for key in dict.fromkeys(target['target_ids'])]
                        used.update(target['target_ids'])
                        previous_key = (tuple(sorted(set(target['target_ids']))), target['subject'])
                        previous = previous_positions.get(previous_key)
                        raw_target = raw
                        for key in target_path:
                            raw_target = raw_target[key]
                        manual_fill = target['manual_fill']
                        if 'manual_fill' not in raw_target and previous:
                            manual_fill = previous.get('manual_fill', False)
                        selection = {field: list(dict.fromkeys(target[field] if field in raw_target else
                            previous_selections.get(previous_key, {}).get(field, [])))
                            for field in ('rule_ids', 'source_ids', 'related_target_ids')}
                        _check_reference_ids(selection['related_target_ids'], mapping,
                            (*target_path, 'related_target_ids'), 'Select supplied response-target references.')
                        additional = rule_refs(selection['rule_ids'], (*target_path, 'rule_ids'))
                        used_rules.update(selection['rule_ids'])
                        _check_reference_ids(target['template_part_ids'], part_map, (*target_path, 'template_part_ids'),
                            'Select template parts provided in this assembly request.', code='outline_unknown_template')
                        template_ids = list(dict.fromkeys([*target['template_part_ids'],
                            *expand_template_spans(part_map, target['template_spans'],
                                repository or EvidenceRepository(artifacts), path=[*target_path, 'template_spans'],
                                all_parts=getattr(context, 'template_parts', part_map))]))
                        if target['template_spans']:
                            template_ids = ordered_template_part_ids({identity: part_map[identity]
                                for identity in template_ids}, repository or EvidenceRepository(artifacts))
                        inherited = [previous] if previous else selected
                        if not template_override(target_path):
                            template_ids = list(dict.fromkeys(part_ids[template_part_key(part)]
                                for entry in inherited for part in entry.get('template_refs', [])))
                        additional_sources = []
                        _check_reference_ids(selection['source_ids'], submission_source_ids, (*target_path, 'source_ids'),
                            'Select sources supplied in this assembly request.', code='outline_unknown_source')
                        _check_reference_ids(selection['source_ids'], source_bindings, (*target_path, 'source_ids'),
                            'This source has no frozen reference; select a bindable source or leave it unresolved.',
                            code='outline_source_unbindable')
                        additional_sources.extend(ref for identity in selection['source_ids'] for ref in source_bindings[identity])
                        exact_sources = [ref for t in selected for ref in t.get('source_excerpts', [])]
                        exact_sources.extend(ref for identity in selection['source_ids']
                                             for ref in submission_excerpt_bindings.get(identity, []))
                        for reference in additional:
                            view = by_ref[_json(reference)]
                            semantic = view.get('item', view.get('effective_rule', {}))
                            additional_sources.extend(ref for e in semantic.get('evidence', [])
                                                      for ref in context.sources.get(e['range_id'], []))
                            exact_sources.extend(ref for e in semantic.get('evidence', [])
                                                 for ref in getattr(context, 'excerpts', {}).get(e['range_id'], []))
                        bound = {'kind': target['kind'], 'purpose': target['purpose'],
                            **({'subject': target['subject']} if target['subject'] else {}),
                            'manual_fill': manual_fill or any(t.get('manual_fill', False) for t in selected),
                            'support_origins': list(dict.fromkeys(origin for t in selected
                                for origin in t.get('support_origins', ['knowledge']))) if selected else target['support_origins'],
                            'rule_refs': list({_json(r): r for r in [
                                *(ref for t in selected for ref in t['rule_refs']), *additional]}.values()),
                            'source_refs': list({_json(r): r for r in [
                                *(ref for t in selected for ref in t['source_refs']), *additional_sources]}.values()),
                            'template_refs': [deepcopy(selected_parts[p]) for p in template_ids]}
                        if exact_sources:
                            bound['source_excerpts'] = list({source_excerpt_key(ref): ref for ref in exact_sources}.values())
                        row_values = []
                        table_context = [deepcopy(ref) for t in inherited
                            for ref in (t.get('response_table') or {}).get('context_source_excerpts', [])]
                        explicit_context = []
                        if target.get('response_table'):
                            if bound['template_refs']:
                                raise ModelContractError([{'path': [*target_path, 'response_table'],
                                    'code': 'outline_conflicting_table',
                                    'detail': 'Select one actual table carrier per position; use distinct targets for distinct deliverables.'}])
                            _check_reference_ids(target['response_table']['context_range_ids'], excerpt_map,
                                (*target_path, 'response_table', 'context_range_ids'),
                                'Select supplied exact source references.')
                            explicit_context = [deepcopy(excerpt_map[i])
                                for i in target['response_table']['context_range_ids']]
                            table_context.extend(explicit_context)
                            for row_number, row in enumerate(target['response_table']['rows']):
                                _check_reference_ids(row['range_ids'], excerpt_map,
                                    (*target_path, 'response_table', 'rows', row_number, 'range_ids'),
                                    'Select supplied exact source references.')
                                row_values.append({'source_excerpts': [deepcopy(excerpt_map[i])
                                    for i in dict.fromkeys(row['range_ids'])],
                                    'rule_refs': rule_refs(row['rule_ids'], (*target_path, 'response_table', 'rows', row_number, 'rule_ids'))})
                                used_rules.update(row['rule_ids'])
                        elif not bound['template_refs']:
                            row_values = [deepcopy(row) for t in inherited for row in
                                          (t.get('response_table') or {}).get('rows', [])]
                        inherited_rows = [row for t in inherited
                                          for row in (t.get('response_table') or {}).get('rows', [])]
                        if inherited_rows and bound['template_refs']:
                            raise ModelContractError([{'path': [*target_path, 'template_part_ids'],
                                'code': 'outline_conflicting_table',
                                'detail': 'Keep known response rows in their business table; select native forms as separate grounded positions.'}])
                        def row_atoms(row):
                            return {(ref['source_ref']['source_index_ref']['sha256'], ref['source_ref']['unit_id'],
                                     s['source_range_id'], s['start'], s['end'])
                                    for ref in row['source_excerpts'] for s in ref['selectors']}
                        covered_atoms = set().union(*(row_atoms(row) for row in row_values)) if row_values else set()
                        context_sets = {frozenset(source_excerpt_key(ref) for ref in
                            t['response_table'].get('context_source_excerpts', []))
                            for t in inherited if t.get('response_table')}
                        if len(context_sets) > 1:
                            assigned_context = {source_excerpt_key(ref) for ref in [*explicit_context,
                                *(ref for row in row_values for ref in row['source_excerpts'])]}
                            if ('context_range_ids' not in (raw_target.get('response_table') or {})
                                    or set().union(*context_sets) - assigned_context
                                    or any(row_atoms(row) - covered_atoms for row in inherited_rows)):
                                known_excerpts = {source_excerpt_key(ref) for t in inherited for ref in [
                                    *t.get('source_excerpts', []),
                                    *(t.get('response_table') or {}).get('context_source_excerpts', []),
                                    *(ref for row in (t.get('response_table') or {}).get('rows', [])
                                      for ref in row['source_excerpts'])]}
                                extra_excerpts = list({source_excerpt_key(ref): ref for ref in [*exact_sources,
                                    *explicit_context, *(ref for row in row_values for ref in row['source_excerpts'])]
                                    if source_excerpt_key(ref) not in known_excerpts}.values())
                                known_sources = {_json(ref) for t in inherited for ref in t['source_refs']}
                                extra_sources = list({_json(ref): ref for ref in [*bound['source_refs'],
                                    *(ref['source_ref'] for ref in extra_excerpts)]
                                    if _json(ref) not in known_sources}.values())
                                known_rules = {_json(ref) for t in inherited for ref in t['rule_refs']}
                                extra_rules = list({_json(ref): ref for ref in [*bound['rule_refs'],
                                    *(ref for row in row_values for ref in row['rule_refs'])]
                                    if _json(ref) not in known_rules}.values())
                                location = '章节「' + c['title'] + '」，原用途：' + '；'.join(t['purpose'] for t in inherited)
                                header_issues.append({'code': 'outline_response_context_unassigned',
                                    'detail': location + '。不同原表的条件及响应行尚未完整交接，已保留各份原表及空应答；'
                                        '新增来源仅保留为理解上下文，归属待核。', 'rule_refs': []})
                                if extra_rules:
                                    header_issues.append({'code': 'outline_rule_unassigned',
                                        'detail': location + '。新增规则尚未确定响应位置，归属待核。', 'rule_refs': extra_rules})
                                for position, (identity, original) in enumerate(zip(
                                        dict.fromkeys(target['target_ids']), inherited)):
                                    retained = deepcopy(original)
                                    if position == 0:
                                        retained['source_refs'] = list({_json(ref): ref for ref in [
                                            *retained['source_refs'], *extra_sources]}.values())
                                        if extra_excerpts:
                                            retained['source_excerpts'] = list({source_excerpt_key(ref): ref for ref in [
                                                *retained.get('source_excerpts', []), *extra_excerpts]}.values())
                                    targets.append(retained)
                                    selections.append((retained, {'related_target_ids': selection['related_target_ids']}))
                                    links.append((retained, selection['related_target_ids']))
                                    owners.setdefault(identity, []).append(retained)
                                continue
                            table_context = explicit_context
                        for row in inherited_rows:
                            if row_atoms(row) - covered_atoms:
                                row_values.append(deepcopy(row))
                                covered_atoms.update(row_atoms(row))
                                header_issues.append({'code': 'outline_response_row_unassigned',
                                    'detail': '组装未完整安排已知响应行，宿主保留其完整要求及空应答，归属待核。',
                                    'rule_refs': row['rule_refs']})
                        if row_values:
                            bound['response_table'] = {'rows': list({_json({k: v for k, v in row.items() if k != 'row_id'}):
                                {**row, 'row_id': 'pending-row'} for row in row_values}.values()),
                                **({'context_source_excerpts': list({source_excerpt_key(ref): ref
                                    for ref in table_context}.values())} if table_context else {})}
                            bound['source_refs'] = list({_json(ref): ref for ref in [*bound['source_refs'],
                                *(ref['source_ref'] for ref in table_context),
                                *(ref['source_ref'] for row in row_values for ref in row['source_excerpts'])]}.values())
                            bound['rule_refs'] = list({_json(ref): ref for ref in [*bound['rule_refs'],
                                *(ref for row in row_values for ref in row['rule_refs'])]}.values())
                        targets.append(bound)
                        selections.append((bound, selection))
                        links.append((bound, selection['related_target_ids']))
                        for identity in dict.fromkeys(target['target_ids']):
                            owners.setdefault(identity, []).append(bound)
                    output.append({'title': c['title'], 'is_volume': c['is_volume'],
                                   'presentation_role': c['presentation_role'],
                                   'targets': targets, 'children': bind(c['children'], (*chapter_path, 'children'))})
                return output
            result = bind(parsed['chapters'])
            excluded, exclusion_issues = set(), list(header_issues)
            added_constraints = rule_refs(parsed['constraint_rule_ids'], ['constraint_rule_ids'])
            used_rules.update(parsed['constraint_rule_ids'])
            for number, entry in enumerate(parsed['excluded_rules']):
                path = ['excluded_rules', number, 'rule_ids']
                excluded_refs = rule_refs(entry['rule_ids'], path)
                _check_reference_ids(entry['rule_ids'], missing_rules.keys() - used_rules, path,
                    'A rule cannot be both assigned and excluded.', code='outline_conflicting_disposition')
                exclusion_issues.append({'code': 'outline_rule_excluded', 'detail': entry['reason'],
                                         'rule_refs': excluded_refs})
            for number, entry in enumerate(parsed['excluded_targets']):
                identities = set(entry['target_ids'])
                path = ['excluded_targets', number, 'target_ids']
                _check_reference_ids(entry['target_ids'], mapping, path,
                                     'Exclude only supplied response-target references.')
                _check_reference_ids(entry['target_ids'], mapping.keys() - used, path,
                    'A candidate cannot be both selected and excluded.', code='outline_conflicting_disposition')
                excluded.update(identities)
                selected = [mapping[key] for key in dict.fromkeys(entry['target_ids'])]
                exclusion_issues.append({'code': 'outline_target_excluded',
                    'detail': entry['reason'] + ' 原候选用途：' + '；'.join(t['purpose'] for t in selected),
                    'rule_refs': list({_json(ref): ref for t in selected for ref in t['rule_refs']}.values())})
            used.update(excluded)
            omitted = {key: deepcopy(t) for key, t in mapping.items() if key not in used}
            for role in ('body', 'submission_note'):
                retained = [target for key, target in omitted.items()
                            if (candidate_roles[key] == 'submission_note') == (role == 'submission_note')]
                if retained:
                    result.append({'title': '待核提交待办' if role == 'submission_note' else '待整理内容',
                        'is_volume': False, 'presentation_role': role, 'targets': retained, 'children': []})
            if omitted:
                owners.update({key: [target] for key, target in omitted.items()})
            # Resolve links after the same host numbering used by the persisted tree.
            for ordinal, chapter in enumerate(_flatten(result)):
                for number, target in enumerate(chapter['targets']):
                    target['_assembly_ordinal'] = (ordinal, number)
            numbered = _renumber(result)
            formal = {target['_assembly_ordinal']: target for c in _flatten(numbered) for target in c['targets']}
            for target, identities in links:
                bound = formal[target['_assembly_ordinal']]
                bound['related_target_ids'] = []
                for identity in dict.fromkeys(identities):
                    candidates = owners.get(identity, [])
                    if len(candidates) != 1:
                        exclusion_issues.append({'code': 'outline_reference_ambiguous',
                            'detail': '跨位置引用未能确定唯一填写实例，保留待核。', 'rule_refs': bound['rule_refs']})
                    else:
                        linked = formal[candidates[0]['_assembly_ordinal']]['target_id']
                        if linked != bound['target_id'] and linked not in bound['related_target_ids']:
                            bound['related_target_ids'].append(linked)
            for target in formal.values():
                target.pop('_assembly_ordinal', None)
            added_intents = []
            for number, intent in enumerate(parsed['search_intents']):
                references = rule_refs(intent['for_ids'], ['search_intents', number, 'for_ids'])
                source_refs = [source for ref in references
                    for e in by_ref[_json(ref)].get('item', by_ref[_json(ref)].get('effective_rule', {})).get('evidence', [])
                    for source in context.sources.get(e['range_id'], [])]
                added_intents.append({'query': intent['query'], 'rule_refs': references,
                    'source_refs': list({_json(ref): ref for ref in source_refs}.values()), 'range_ids': []})
            candidate_bindings = {}
            for alias, selected in owners.items():
                for target in selected:
                    candidate_bindings.setdefault(formal[target['_assembly_ordinal']]['target_id'], []).append(alias)
            candidate_selections = {formal[target['_assembly_ordinal']]['target_id']: selection
                                    for target, selection in selections}
            return (numbered, bool(omitted), exclusion_issues, {origins[key] for key in excluded},
                    added_constraints, added_intents, candidate_bindings, candidate_selections)
        assembled = await _ask(execute, budget, 'outline', payload, assemble)
        (chapters, omitted, exclusion_issues, excluded_origins, added_constraints, added_intents,
         candidate_bindings, candidate_selections) = assembled
        refs = list({_json(ref): ref for ref in [*refs, *added_constraints]}.values())
        if omitted:
            gaps.append(_gap('outline_assembly_incomplete', '归并未安排的响应位置已保留在末尾，顺序待核对。'))
    except _LOCAL_FAILURES:
        exclusion_issues = []
        excluded_origins = set()
        added_intents = []
        candidate_bindings = {}
        candidate_selections = {}
        chapters = []
        for role in ('body', 'submission_note'):
            retained = [deepcopy(target) for key, target in mapping.items()
                        if (candidate_roles[key] == 'submission_note') == (role == 'submission_note')]
            if retained:
                chapters.append({'title': '待核提交待办' if role == 'submission_note' else '待整理内容',
                    'is_volume': False, 'presentation_role': role, 'children': [], 'targets': retained})
        gaps.append(_gap('outline_assembly_unavailable', '正式归并未完成，已保留待整理总稿及全部响应位置，不宣称分卷已完成。'))
    def finalize(chapters, exclusion_issues, excluded_origins, added_intents):
        issues = [{**i, 'detail': '来源读取阶段观察（最终情况以全局核对为准）：' + i['detail']}
                  for ordinal, f in enumerate(fragments) for i in f['issues']
                  if i['code'] != 'outline_rule_unassigned'
                  and (ordinal, i.get('target_id')) not in excluded_origins]
        issues.extend(exclusion_issues)
        assigned = {_json(r) for c in _flatten(chapters) for t in c['targets'] for r in t['rule_refs']}
        assigned.update(_json(r) for r in refs)
        assigned.update(_json(ref) for issue in exclusion_issues for ref in issue['rule_refs'])
        for ref in context.rules.values():
            if _json(ref) not in assigned:
                issues.append({'code': 'outline_rule_unassigned', 'detail': '该规则尚未安排响应或交付位置。', 'rule_refs': [ref]})
        # Local path IDs cannot survive assembly. Issues remain truthful at outline level.
        for issue in issues:
            issue.pop('target_id', None)
        return AuthoringOutline(schema_version=2, authoring_input=context.input, chapters=_renumber(chapters),
                                issues=issues, constraint_rule_refs=refs, submission_refs=submission_refs,
                                search_intents=list({_json(intent): intent for intent in [
                                    *(intent for f in fragments for intent in f.get('search_intents', [])),
                                    *added_intents]}.values())).model_dump(exclude_unset=True)
    outline = finalize(chapters, exclusion_issues, excluded_origins, added_intents)
    if structure_results is not None:
        from docreader.tender.authoring_structure import review_structure
        findings, structure_gaps = await review_structure(
            artifacts, outline, [], budget, execute, outline_phase=True,
            apply_source_decoration_exclusions=True)
        if findings:
            choices = {}
            for chapter in _flatten(outline['chapters']):
                for target in chapter['targets']:
                    aliases = candidate_bindings.get(target['target_id'], [])
                    if aliases:
                        key = (tuple(sorted(aliases)), target.get('subject'))
                        choices.setdefault(key, []).append(target)
            previous_positions = {key: values[0] for key, values in choices.items() if len(values) == 1}
            previous_selections = {key: candidate_selections.get(target['target_id'], {})
                                   for key, target in previous_positions.items()}
            previous_targets = {target['target_id']: f'v{ordinal}' for ordinal, target in enumerate(
                (t for c in _flatten(outline['chapters']) for t in c['targets']), 1)}

            def previous_structure(entries):
                return [{'chapter_id': chapter['chapter_id'], 'title': chapter['title'],
                         'is_volume': chapter.get('is_volume', False),
                         'presentation_role': chapter.get('presentation_role', 'body'),
                         'targets': [{'review_target_id': previous_targets[target['target_id']],
                                      'candidate_target_ids': candidate_bindings.get(target['target_id'], []),
                                      'kind': target['kind'], 'purpose': target['purpose'],
                                      'manual_fill': target.get('manual_fill', False),
                                      **candidate_selections.get(target['target_id'], {}),
                                      **({'subject': target['subject']} if target.get('subject') else {}),
                                      'template_part_ids': [part_ids[template_part_key(part)]
                                          for part in target.get('template_refs', [])],
                                      **({'response_table': {'context_range_ids': [excerpt_ids[source_excerpt_key(ref)]
                                              for ref in target['response_table'].get('context_source_excerpts', [])],
                                          'rows': [{'range_ids': [excerpt_ids[source_excerpt_key(ref)]
                                                        for ref in row['source_excerpts']],
                                                    'rule_ids': [identity for ref in row['rule_refs']
                                                        for identity, value in assembly_rules.items() if value == ref]}
                                                   for row in target['response_table']['rows']]}}
                                         if target.get('response_table') else {})}
                                     for target in chapter['targets']],
                         'children': previous_structure(chapter['children'])} for chapter in entries]

            feedback = {**payload, 'previous_structure': previous_structure(outline['chapters']),
                        'review_findings': findings}
            try:
                repaired, omitted, new_issues, new_excluded, new_constraints, new_intents, _, _ = await _ask(
                    execute, budget, 'outline', feedback, assemble)
                refs = list({_json(ref): ref for ref in [*refs, *new_constraints]}.values())
                candidate = finalize(repaired, new_issues, new_excluded, new_intents)
                changed = candidate['chapters'] != outline['chapters']
                outline = candidate
                if changed:
                    new_findings, new_gaps = await review_structure(
                        artifacts, outline, [], budget, execute, outline_phase=True,
                        apply_source_decoration_exclusions=True)
                    if any(g['code'] == 'structure_review_unavailable' for g in new_gaps):
                        findings = [{**f, 'can_revise': False,
                            'detail': '结构修订后尚未复核的上一版观察：' + f['detail']} for f in findings]
                    else:
                        findings = new_findings
                    structure_gaps.extend(new_gaps)
                if omitted:
                    gaps.append(_gap('outline_assembly_incomplete', '结构修订仍有未安排位置，保留待整理内容。'))
            except _LOCAL_FAILURES as error:
                structure_gaps.append(_gap('outline_revision_unavailable',
                    f'本次结构修订未完成（{type(error).__name__}），保留已有大纲及意见继续生成。'))
        structure_results.update(findings=findings, gaps=structure_gaps)
        outline['issues'].extend({'code': finding['code'],
            'detail': finding['detail'] + (' 建议：' + finding['suggestion'] if finding['suggestion'] else ''),
            'rule_refs': []} for finding in findings)
    return outline


def plan_writing_tasks(outline):
    result = []
    identities = {target['target_id'] for chapter in _flatten(outline['chapters']) for target in chapter['targets']}
    def walk(chapters, titles=()):
        for chapter in chapters:
            if chapter.get('presentation_role') == 'submission_note':
                continue
            path, group = (*titles, chapter['title']), []
            def emit():
                if group:
                    result.append(WritingTask(task_id=f'w{len(result) + 1}', chapter_id=chapter['chapter_id'],
                                              title_path=list(path), targets=deepcopy(group)).model_dump(exclude_unset=True))
                    group.clear()
            for target in chapter['targets']:
                related = target.get('related_target_ids', [])
                if (target['kind'] == 'narrative' and not target.get('manual_fill', False)
                        and not any(target.get(field) for field in ('rule_refs', 'source_refs', 'template_refs'))
                        and related and all(identity in identities and identity != target['target_id']
                                            for identity in related)):
                    continue
                if target['kind'] != 'narrative' or target.get('manual_fill', False):
                    emit()
                    group.append(target)
                    emit()
                else:
                    group.append(target)
            emit()
            walk(chapter['children'], path)
    walk(outline['chapters'])
    return result


def _load_task(artifacts, context, task, basis, materials):
    rules, sources, issues, seed_ranges = [], {}, [], set()
    repository = getattr(context, 'repository', None)
    source_options = {'repository': repository} if repository is not None else {}
    template_supports, response_context_supports, response_rows = {}, {}, []
    views = {r['rule_id']: r for r in context.payload['rules']}
    by_ref = {_json(ref): views[identity] for identity, ref in context.rules.items()}
    needed = {m['material_id']: m for m in _task_materials(materials, task)}
    global_issues = [i['issue_id'] for i in context.payload.get('issues', [])
                     if not i.get('range_id') and not i.get('item_ids') and i.get('issue_id')]

    def references(identity):
        return getattr(context, 'excerpts', {}).get(identity) or context.sources.get(identity, [])

    def register(ref, target_alias, *, template=None):
        exact = 'selectors' in ref and template is None
        precise = ref if exact else ({'source_ref': ref, 'selectors': template['selectors']}
                                    if template and template['selectors'] else None)
        key = ('excerpt', source_excerpt_key(precise)) if precise else _json(ref)
        if key not in sources:
            try:
                loaded = (load_source_excerpt(artifacts, ref, **source_options) if exact else
                          load_template_part(artifacts, template, **source_options) if template else
                          load_outline_source(artifacts, ref, **source_options))
                unit = loaded['unit']
                text, ocr = loaded.get('text', unit.get('text', '')), loaded.get('ocr')
                if ocr and ocr.get('text', '').strip() and ocr['text'] != text:
                    text = '\n'.join(part for part in (text, ocr['text']) if part)
                sources[key] = {'support_id': f's{len(sources) + 1}', 'origin': 'tender',
                    'reference': precise or ref, 'target_ids': [], 'text': text,
                    'grid': loaded.get('grid', unit.get('grid')),
                    'document_role': loaded['document'].get('role'),
                    'heading_path': unit.get('heading_path', unit.get('locator', {}).get('heading_path', ''))}
                if ocr:
                    sources[key]['ocr'] = {k: v for k, v in ocr.items() if k != 'text'}
            except (OSError, ValueError, KeyError):
                issues.append('相关原文或模板暂不可读取，保留待核对位置。')
                return None
        if target_alias not in sources[key]['target_ids']:
            sources[key]['target_ids'].append(target_alias)
        return sources[key]['support_id']

    for ordinal, target in enumerate(task['targets'], 1):
        alias = f't{ordinal}'
        target_rules = [by_ref[_json(ref)] for ref in target['rule_refs'] if _json(ref) in by_ref]
        rules.extend(target_rules)
        rows = (target.get('response_table') or {}).get('rows', [])
        table_context = (target.get('response_table') or {}).get('context_source_excerpts', [])
        excerpts = [*target.get('source_excerpts', []),
                    *table_context,
                    *(ref for row in rows for ref in row['source_excerpts'])]
        precise_units = {_json(ref['source_ref']) for ref in excerpts}
        coarse_refs = [ref for ref in target['source_refs'] if _json(ref) not in precise_units]
        refs = [*excerpts, *coarse_refs]
        original_ranges = {selected['range_id'] for ref in excerpts for selected in ref['selectors']}
        original_ranges.update(identity for identity, values in context.sources.items()
                               if any(ref in coarse_refs for ref in values))
        for part in target.get('template_refs', []):
            if part['selectors']:
                original_ranges.update(selected['range_id'] for selected in part['selectors'])
            else:
                original_ranges.update(identity for identity, values in context.sources.items()
                                       if part['source_ref'] in values)
        for rule in target_rules:
            semantic = rule.get('item', rule.get('effective_rule', {}))
            for evidence in semantic.get('evidence', []):
                original_ranges.add(evidence['range_id'])
                refs.extend(references(evidence['range_id']))
        seed_ranges.update(original_ranges)
        associated = _analysis_context(context, target_rules, original_ranges, global_issues)

        def collect_references(value):
            if isinstance(value, dict):
                if 'range_id' in value:
                    refs.extend(references(value['range_id']))
                for entry in value.values():
                    collect_references(entry)
            elif isinstance(value, list):
                for entry in value:
                    collect_references(entry)

        collect_references(associated)
        for ref in refs:
            register(ref, alias)
        response_context_supports[alias] = list(dict.fromkeys(identity for ref in table_context
            if (identity := register(ref, alias))))
        template_supports[alias] = []
        for part in target.get('template_refs', []):
            identity = register(part['source_ref'], alias, template=part)
            if identity:
                template_supports[alias].append(identity)
        for row in rows:
            identities = [register(ref, alias) for ref in row['source_excerpts']]
            response_rows.append({'row_id': f'q{len(response_rows) + 1}', 'target_id': alias,
                'rule_ids': [by_ref[_json(ref)]['rule_id'] for ref in row['rule_refs'] if _json(ref) in by_ref],
                'support_ids': list(dict.fromkeys(identity for identity in identities if identity))})
    material_aliases = {identity: f'k{i}' for i, identity in enumerate(needed, 1)}
    shared = {**basis['response'], 'project_background': '', 'approach': '',
              'shared_arrangements': [{**{k: v for k, v in entry.items() if k != 'material_ids'},
                                       'support_ids': [material_aliases[m] for m in entry['material_ids']]}
                  for entry in basis['response']['shared_arrangements']
                  if entry['material_ids'] and set(entry['material_ids']) <= needed.keys()]}
    supports = [{k: v for k, v in s.items() if k != 'reference'} for s in sources.values()]
    supports.extend(_material_views(list(needed.values()), task['targets']))
    payload = {'grounding_policy': AUTHORING_GROUNDING_POLICY, 'title_path': task['title_path'],
               'targets': [{'target_id': f't{i + 1}', 'kind': t['kind'], 'purpose': t['purpose'],
                            **({'subject': t['subject']} if t.get('subject') else {}),
                            'manual_fill': t.get('manual_fill', False),
                            'support_origins': t.get('support_origins', ['knowledge']),
                            'template_support_ids': template_supports[f't{i + 1}'],
                            **({'response_context_support_ids': response_context_supports[f't{i + 1}']}
                               if response_context_supports[f't{i + 1}'] else {}),
                            'rule_ids': [by_ref[_json(ref)]['rule_id'] for ref in t['rule_refs'] if _json(ref) in by_ref],
                            'support_ids': [s['support_id'] for s in supports if f't{i + 1}' in s['target_ids']
                                            and s['origin'] in t.get('support_origins', ['knowledge'])]}
                           for i, t in enumerate(task['targets'])],
               'rules': list({_json(r): r for r in rules}.values()),
               'rule_origin': context.payload.get('rule_origin', 'extraction'),
               'supports': supports, 'basis': shared,
               'input_issues': context.input['issues'], 'source_issues': issues,
               **_analysis_context(context, rules, seed_ranges, global_issues)}
    if response_rows:
        payload['response_rows'] = response_rows
    return payload, {s['support_id']: s['reference'] for s in sources.values()}, issues


def bind_writing_response(task, raw, sources, materials, *, support_views=None):
    response = _parse(WritingResponse, raw)
    gaps, blocks, fills, row_fills = list(response['gaps']), [], [], {}
    targets = {f't{i + 1}': target for i, target in enumerate(task['targets'])}
    selected = {m['material_id']: m for m in materials}
    bindings = _support_bindings(sources, materials)
    if support_views is None:
        support_views = [*_material_views(materials, task['targets']),
                         *({'support_id': identity, 'origin': 'tender',
                            'target_ids': [key for key, target in targets.items() if ref in target['source_refs']]}
                           for identity, ref in sources.items())]
    views = {s['support_id']: s for s in support_views}

    def applies(identity, target_id):
        support, target = views.get(identity, {}), targets[target_id]
        return (not target.get('manual_fill', False)
                and support.get('origin') in target.get('support_origins', ['knowledge'])
                and target_id in support.get('target_ids', []))

    def grounded(content):
        target_ids = list(dict.fromkeys(content['target_ids']))
        support_ids = list(dict.fromkeys(content['support_ids']))
        if (not target_ids or not support_ids or set(target_ids) - targets.keys()
                or set(support_ids) - bindings.keys()):
            gaps.append('本处生成应答缺少可绑定的目标或资料依据，未采用，响应留空。')
            return None
        linked = [targets[i] for i in target_ids]
        if (any(not any(applies(s, t) for s in support_ids) for t in target_ids)
                or any(not any(applies(s, t) for t in target_ids) for s in support_ids)):
            gaps.append('本处生成应答引用了不属于该目标的资料或人工填写位置，未采用，响应留空。')
            return None
        return {**{k: v for k, v in content.items() if k != 'support_ids'},
                'target_ids': [t['target_id'] for t in linked],
                'material_ids': [bindings[s]['material_id'] for s in support_ids if bindings[s]['origin'] == 'knowledge'],
                'source_refs': list({_json(bindings[s]['source_ref']): bindings[s]['source_ref']
                    for s in support_ids if bindings[s]['origin'] == 'tender'}.values()),
                'source_excerpts': [bindings[s]['source_excerpt'] for s in support_ids
                    if bindings[s]['origin'] == 'tender' and bindings[s].get('source_excerpt')]}

    for block in response['blocks']:
        bound = grounded(block)
        if bound is not None:
            if any(t['kind'] == 'form' for t in task['targets'] if t['target_id'] in bound['target_ids']):
                gaps.append('表单使用已选原格式和格填值，未另采用重写正文；相关说明应安排独立正文位置。')
                continue
            blocks.append(bound)
    for fill in response['cell_fills']:
        if fill['source_id'] not in sources:
            gaps.append('填写值未能绑定原模板，请人工定位：' + fill['text'])
            continue
        bound = grounded(fill)
        if bound is not None:
            linked = [t for t in task['targets'] if t['target_id'] in bound['target_ids']]
            if any(t['kind'] != 'form' or sources[fill['source_id']] not in
                   [part['source_ref'] for part in t.get('template_refs', [])] for t in linked):
                gaps.append('填写值的模板不属于其引用的响应位置，未采用，原表对应单元格留空。')
                continue
            fills.append({'source_ref': sources[fill['source_id']],
                          **{k: v for k, v in bound.items() if k != 'source_id'}})
    rows = {f'q{ordinal}': (f't{number}', row) for ordinal, (number, row) in enumerate(
        ((number, row) for number, target in enumerate(task['targets'], 1)
         for row in (target.get('response_table') or {}).get('rows', [])), 1)}
    conflicted_rows = set()
    for fill in response.get('row_fills', []):
        if fill['row_id'] not in rows:
            gaps.append('企业应答未绑定本次技术响应行，未采用；原要求保留。')
            continue
        alias, row = rows[fill['row_id']]
        if (not fill['text'].strip() or not any(bindings.get(s, {}).get('origin') == 'knowledge'
                and applies(s, alias) for s in fill['support_ids'])):
            if fill['text'].strip():
                gaps.append('技术响应行没有适用的企业资料支持，应答留空，原要求保留。')
            continue
        value = grounded({**fill, 'target_ids': [alias]})
        if value is None:
            continue
        value = {k: v for k, v in value.items() if k not in {'target_ids', 'row_id'}}
        value['row_id'] = row['row_id']
        previous = row_fills.get(row['row_id'])
        if previous is not None and previous != value:
            conflicted_rows.add(row['row_id'])
            gaps.append('同一技术响应行收到冲突应答，该格留空待核，原要求保留。')
        row_fills[row['row_id']] = value
    row_fills = [value for key, value in row_fills.items() if key not in conflicted_rows]
    attachments = []
    for alias in dict.fromkeys(response['attachment_ids']):
        identity = bindings.get(alias, {}).get('material_id')
        material = selected.get(identity)
        if material is None:
            gaps.append('模型选择了未提供的资料引用，未将其作为证明。')
        elif (material['attachment'] and material['original_ref'] and material['text'].strip() and
              any(_material_applies(material, t) and (t['kind'] == 'attachment'
                  or material.get('locator', {}).get('kind') != 'search_excerpt') for t in task['targets'])):
            attachments.append({'material_id': identity, 'title': material['title'], 'artifact_ref': material['original_ref']})
    covered = {identity for content in [*blocks, *fills] for identity in content['target_ids']}
    answered = {fill['row_id'] for fill in row_fills}
    covered.update(target['target_id'] for target in task['targets']
                   if any(row['row_id'] in answered for row in (target.get('response_table') or {}).get('rows', [])))
    covered.update(t['target_id'] for t in task['targets'] if t['kind'] == 'attachment' and
                   any(_material_applies(selected[a['material_id']], t) for a in attachments))
    for target in task['targets']:
        if target['target_id'] not in covered:
            gaps.append('本响应位置未取得有依据的应答，保留空位：' + target['purpose'])
    has_content = _has_content({'blocks': blocks, 'cell_fills': fills, 'row_fills': row_fills, 'attachments': attachments})
    if not has_content:
        gaps.append('本响应位置尚无实际正文、填写值或证明原件，待补充。')
    return WritingUnit(task_id=task['task_id'], chapter_id=task['chapter_id'],
                       target_ids=[t['target_id'] for t in task['targets']],
                       status='generated' if has_content else 'placeholder', blocks=blocks,
                       cell_fills=fills, row_fills=row_fills, attachments=attachments, gaps=gaps).model_dump()


def _has_content(unit):
    return bool(any(b['text'].strip() or any(h.strip() for h in b['headers']) or
                    any(any(c.strip() for c in row) for row in b['rows']) for b in unit['blocks'])
                or any(f['text'].strip() for f in [*unit['cell_fills'], *unit.get('row_fills', [])])
                or unit['attachments'])


def _review_record(task, unit, payload, sources, materials):
    if (not _has_content(unit) and not materials
            and all(target['kind'] == 'form' for target in task['targets'])):
        return None
    from docreader.tender.authoring_revision import _previous_content
    content = _previous_content(task, unit, payload, sources, _support_bindings(sources, materials))
    if content is None:
        unit['gaps'].append('已有内容的依据无法绑定本次复核，保留草稿待核。')
        return None
    return {'task_id': task['task_id'], 'title_path': task['title_path'],
            'rules': payload['rules'], 'supports': payload['supports'], 'targets': payload['targets'], 'basis': payload['basis'],
            **{key: payload[key] for key in ('analysis_issues', 'issue_resolutions', 'relations',
                                           'unverified_relation_ids', 'related_rules')},
            **({'response_rows': payload['response_rows']} if payload.get('response_rows') else {}),
            'content': {'status': unit['status'], **content}}


def _content_rejected(finding):
    return finding.get('reject_content') is True or finding['code'] == 'unsupported_content'


def _adopt_reviewed(units, findings, artifacts, superseded):
    """Remove explicit semantic rejections from the version that was reviewed."""
    result = []
    for unit in units:
        rejected = [f for f in findings if unit['task_id'] in f['task_ids'] and _content_rejected(f)]
        if not rejected:
            result.append(unit)
            continue
        value = deepcopy(unit)
        clear = any(not f.get('content_ids') for f in rejected)
        ids = {i for f in rejected for i in f.get('content_ids', [])}
        value['blocks'] = [b for i, b in enumerate(unit['blocks'], 1) if not clear and f'b{i}' not in ids]
        value['cell_fills'] = [f for i, f in enumerate(unit['cell_fills'], 1) if not clear and f'f{i}' not in ids]
        value['row_fills'] = [f for i, f in enumerate(unit.get('row_fills', []), 1) if not clear and f'r{i}' not in ids]
        value['attachments'] = [a for i, a in enumerate(unit['attachments'], 1) if not clear and f'a{i}' not in ids]
        value['status'] = 'generated' if _has_content(value) else 'placeholder'
        value['gaps'].append('复核明确否定的内容未采用，相应位置留空；其他有据内容保留。')
        if value != unit:
            superseded.append(artifacts.put_json(unit))
        result.append(value)
    return result


async def generate_document(artifacts, snapshot, budget, execute, *, materials=(), knowledge=None,
                            concurrency=2, review_request=None, submission_sources=(), on_outline=None,
                            outline=None, outline_only=False):
    """Plan an outline or compile its fixed tree; the executor supplies document usage."""
    snapshot = AuthoringInput.model_validate(snapshot).model_dump()
    supplied_outline = outline is not None
    if supplied_outline:
        parsed = AuthoringOutline.model_validate(outline)
        if parsed.authoring_input.model_dump() != snapshot:
            raise ValueError('fixed outline belongs to another authoring snapshot')
        outline = parsed.model_dump(exclude_unset=True)
        outline['authoring_input'] = deepcopy(snapshot)
    if review_request is not None:
        from docreader.tender.schema import AuthoringReviewRequest
        review_request = AuthoringReviewRequest.model_validate(review_request).model_dump()
    context = await asyncio.to_thread(_OutlineContext, artifacts, snapshot)
    materials, gaps = ([], []) if outline_only else prepare_materials(artifacts, materials)
    gaps.extend(_gap(i['code'], i['detail']) for i in context.availability_issues)
    gaps.extend(_gap(i['code'], '上游分析待核对：' + i['detail']) for i in context.payload.get('issues', [])
                if i.get('issue_id'))
    structure_results = {'findings': [], 'gaps': []}
    if not supplied_outline:
        organization_options = {'submission_sources': submission_sources} if submission_sources else {}
        outline = await _organize(artifacts, context, budget, execute, gaps,
                                  structure_results=structure_results, **organization_options)
    elif not outline_only:
        # Rebuild the same paragraph references registered during automatic organization.
        await asyncio.to_thread(prepare_template_parts, artifacts, context)
        from docreader.tender.authoring_structure import review_structure
        findings, structure_gaps = await review_structure(
            artifacts, outline, [], budget, execute, outline_phase=True)
        structure_results.update(findings=findings, gaps=structure_gaps)
    gaps.extend(structure_results['gaps'])
    if not any(c['targets'] for c in _flatten(outline['chapters'])):
        gaps.append(_gap('outline_empty', '当前大纲没有实际响应位置，保留待补正文；不代表已覆盖招标要求。'))
        if not supplied_outline:
            outline['chapters'].append({'chapter_id': f"chapter-{len(outline['chapters']) + 1}",
                'title': '待补响应内容', 'targets': [{'target_id': 'pending-response', 'kind': 'narrative',
                    'purpose': '仅依据可用资料起草方法，完整招标要求和响应结构待补充。', 'rule_refs': [], 'source_refs': []}],
                'children': [], 'is_volume': False})
    if outline_only:
        issues = [*outline['issues'], *({'code': gap['code'], 'detail': gap['detail'], 'rule_refs': [],
                  **({'target_id': gap['target_id']} if gap.get('target_id') else {})} for gap in gaps)]
        outline['issues'] = list({_json(issue): issue for issue in issues}.values())
    outline_ref = artifacts.put_json(outline)
    if on_outline is not None:
        await on_outline(outline_ref)
    if outline_only:
        return outline_ref
    from docreader.tender.authoring_requirements import preserve_requirements
    requirements = await asyncio.to_thread(preserve_requirements, context, outline)
    requirements_ref = artifacts.put_json(requirements)
    gaps.extend(_gap(issue['code'], issue['detail']) for issue in requirements['issues'])
    tasks = plan_writing_tasks(outline)
    requested = set(review_request['target_ids']) if review_request else set()
    available = {target['target_id'] for task in tasks for target in task['targets']}
    if requested - available:
        gaps.append(_gap('review_request_unbound', '部分重审位置未能绑定当前大纲，保留待核；其余位置继续。'))
    if knowledge is not None:
        queries = _knowledge_queries(outline, tasks, gaps)
        retrieved = await knowledge.search(queries, artifacts)
        combined, issues = prepare_materials(artifacts, [*materials, *retrieved['materials']])
        materials = combined
        gaps.extend(issues)
        gaps.extend({k: v for k, v in issue.items() if k in {'code', 'detail', 'target_id'}}
                    for issue in retrieved['issues'])
    # Summarize supplied methods only. Tender requirements are never bidder proof.
    basis_response = AuthoringBasisContent().model_dump()
    candidates = [m for m in materials if m['text'].strip() and
                  any(_material_applies(m, t) for task in tasks for t in task['targets'])]
    basis_targets = [(task['title_path'], target) for task in tasks for target in task['targets']
                     if not target.get('manual_fill', False)]
    rule_views = {r['rule_id']: r for r in context.payload['rules']}
    by_ref = {_json(ref): rule_views[identity] for identity, ref in context.rules.items()}
    material_aliases = {m['material_id']: f'k{i}' for i, m in enumerate(candidates, 1)}
    by_alias = {alias: identity for identity, alias in material_aliases.items()}
    basis_views = _material_views(candidates, [t for _, t in basis_targets])
    basis_records = [{'support': view} for view in basis_views]
    def basis_payload(batch):
        selected = {row['support']['support_id'] for row in batch}
        batch_materials = {by_alias[alias] for alias in selected}
        related = [(i, path, target) for i, (path, target) in enumerate(basis_targets, 1)
                   if any(m['material_id'] in batch_materials and _material_applies(m, target) for m in candidates)]
        rules = list({_json(ref): by_ref[_json(ref)] for _, _, t in related for ref in t['rule_refs']
                      if _json(ref) in by_ref}.values())
        return {'grounding_policy': AUTHORING_GROUNDING_POLICY,
                'supports': [view for view in basis_views if view['support_id'] in selected],
                'input_issues': snapshot['issues'],
                'rules': rules,
                'targets': [{'target_id': f't{i}', 'title_path': path, 'kind': t['kind'], 'purpose': t['purpose'],
                             'rule_ids': [by_ref[_json(ref)]['rule_id'] for ref in t['rule_refs'] if _json(ref) in by_ref],
                             'support_ids': [material_aliases[m['material_id']] for m in candidates
                                              if m['material_id'] in batch_materials and _material_applies(m, t)]}
                            for i, path, t in related],
                'rule_origin': context.payload.get('rule_origin', 'extraction'),
                **_analysis_context(context, rules)}
    # Each batch reads its own evidence once. The host retains previously bound
    # arrangements instead of repeatedly asking the model to copy and recheck them.
    basis_batches = list(_batches(basis_records, budget, 'authoring_basis', basis_payload))
    for batch in basis_batches:
        try:
            def validate_basis(raw):
                value = _parse(AuthoringBasisResponse, raw)
                value['project_background'] = value['approach'] = ''
                if len(_json(value).encode()) > 12000:
                    raise ModelContractError([{'path': [], 'code': 'basis_summary_too_large',
                        'detail': 'Keep the shared proposal within 12000 UTF-8 bytes; detailed requirements remain in writing tasks.'}])
                allowed = {s['support_id'] for s in basis_payload(batch)['supports']}
                arrangements = []
                for entry in value['shared_arrangements']:
                    if not entry['support_ids'] or set(entry['support_ids']) - allowed:
                        value['gaps'].append('一项共同安排缺少可用资料引用，未采用，相关应答留空。')
                        continue
                    arrangements.append({**{k: v for k, v in entry.items() if k != 'support_ids'},
                                         'material_ids': list(dict.fromkeys(by_alias[s] for s in entry['support_ids']))})
                return AuthoringBasisContent(**{**value, 'shared_arrangements': arrangements}).model_dump()
            value = await _ask(execute, budget, 'authoring_basis', basis_payload(batch), validate_basis)
            basis_response['shared_arrangements'] = list({_json(entry): entry for entry in [
                *basis_response['shared_arrangements'], *value['shared_arrangements']]}.values())
            basis_response['gaps'].extend(value['gaps'])
        except _LOCAL_FAILURES as error:
            gaps.append(_gap('basis_partial', f'部分共同依据未形成（{type(error).__name__}）；保留已有方案和待补信息。'))
    known_materials = {m['material_id'] for m in candidates}
    supported = []
    for arrangement in basis_response['shared_arrangements']:
        if not arrangement['material_ids'] or set(arrangement['material_ids']) - known_materials:
            basis_response['gaps'].append('一项共同安排缺少可用资料引用，未采用，相关应答留空。')
        else:
            supported.append(arrangement)
    basis_response['shared_arrangements'] = supported
    basis = AuthoringBasis(authoring_input=snapshot, response=basis_response, materials=materials).model_dump()
    basis_ref = artifacts.put_json(basis)
    gaps.extend(_gap('basis_gap', detail) for detail in basis_response['gaps'] if detail.strip())
    # Keep forms as independent instances. Narrative may split at a target boundary
    # when its complete context does not fit; it is never split by page count.
    from docreader.tender.authoring_fragments import split_writing_task
    planned = []

    async def fit_task(task):
        if all(t.get('manual_fill', False) for t in task['targets']):
            planned.append(task)
            return
        payload, sources, _ = await asyncio.to_thread(_load_task, artifacts, context, task, basis, materials)
        if not _has_allowed_support(payload):
            planned.append(task)
            return
        hit, _ = await _replay(execute, 'authoring_write', _request(budget, 'authoring_write', payload),
            lambda raw: bind_writing_response(task, raw, sources, _task_materials(materials, task),
                                               support_views=payload['supports']))
        if hit or await asyncio.to_thread(_fits, budget, 'authoring_write', payload):
            planned.append(task)
            return
        if len(task['targets']) > 1:
            middle = len(task['targets']) // 2
            parts = [{**task, 'targets': task['targets'][:middle]}, {**task, 'targets': task['targets'][middle:]}]
        else:
            parts = split_writing_task(task)
        if not parts:
            planned.append(task)  # write_one keeps the original carrier and a located capacity gap.
            return
        for part in parts:
            scope = hashlib.sha256(_json(part['targets']).encode()).hexdigest()[:16]
            part['task_id'] = task['task_id'].split(':part-')[0] + ':part-' + scope
            await fit_task(part)

    for task in tasks:
        await fit_task(task)
    async def write_one(task):
        payload, sources, source_issues = await asyncio.to_thread(_load_task, artifacts, context, task, basis, materials)
        if all(t.get('manual_fill', False) for t in task['targets']):
            unit = WritingUnit(task_id=task['task_id'], chapter_id=task['chapter_id'],
                target_ids=[t['target_id'] for t in task['targets']], status='placeholder',
                blocks=[{'kind': 'paragraph', 'text': t['purpose'] + '\n________（人工填写）'}
                        for t in task['targets'] if t['kind'] != 'form'],
                gaps=['此处为人工填写位置；最终报价等数据留空，不由模型代填或按0计算。']).model_dump()
            unit['gaps'].extend(source_issues)
            return unit, None
        if all(t['kind'] == 'form' and not t.get('response_table') for t in task['targets']) and not any(
                support.get('grid') and support['support_id'] in target.get('template_support_ids', [])
                for target in payload['targets'] for support in payload['supports']):
            unit = WritingUnit(task_id=task['task_id'], chapter_id=task['chapter_id'],
                target_ids=[t['target_id'] for t in task['targets']], status='placeholder',
                gaps=['文字原格式保留空位，或实际原表尚未定位；不另外生成一份重写声明。', *source_issues]).model_dump()
            return unit, None
        if (all(t['kind'] == 'attachment' for t in task['targets']) and
                not any(s.get('attachment') for s in payload['supports'] if s['origin'] == 'knowledge')):
            unit = WritingUnit(task_id=task['task_id'], chapter_id=task['chapter_id'],
                target_ids=[t['target_id'] for t in task['targets']], status='placeholder',
                gaps=['当前响应位置没有可用的真实证明原件，保留附件位置待人工补充。']).model_dump()
            unit['gaps'].extend(source_issues)
            return unit, None
        if not _has_allowed_support(payload):
            unit = WritingUnit(task_id=task['task_id'], chapter_id=task['chapter_id'],
                target_ids=[t['target_id'] for t in task['targets']], status='placeholder',
                gaps=['未取得本位置允许使用的可读依据，不生成无依据应答；原模板及响应位置保留空白。']).model_dump()
            unit['gaps'].extend(source_issues)
            return unit, None
        try:
            unit = await _ask(execute, budget, 'authoring_write', payload,
                              lambda raw: bind_writing_response(task, raw, sources,
                                  _task_materials(materials, task), support_views=payload['supports']))
        except _LOCAL_FAILURES as error:
            unit = WritingUnit(task_id=task['task_id'], chapter_id=task['chapter_id'],
                               target_ids=[t['target_id'] for t in task['targets']], status='placeholder',
                               error_type=type(error).__name__, gaps=['本单元生成未完成，已保留待补位置。']).model_dump()
        unit['gaps'].extend(source_issues)
        record = None if unit.get('error_type') else _review_record(
            task, unit, payload, sources, _task_materials(materials, task))
        return unit, record
    limiter = asyncio.Semaphore(concurrency)
    async def bounded(task):
        async with limiter:
            return await write_one(task)
    pending = [asyncio.create_task(bounded(task)) for task in planned]
    try:
        written = await asyncio.gather(*pending)
    except BaseException:
        for work in pending:
            work.cancel()
        await asyncio.gather(*pending, return_exceptions=True)
        raise
    units, unit_refs, review_records = [], [], []
    for task, (unit, review_record) in zip(planned, written):
        units.append(unit)
        unit_refs.append(artifacts.put_json(unit))
        if review_record is not None:
            review_records.append(review_record)
    def review_scope(task):
        return [f't{i}' for i, target in enumerate(task['targets'], 1) if target['target_id'] in requested]
    async def review(records, previous=()):
        findings, completed, replaced = [], set(), set()
        def review_payload(batch):
            identities = {r['task_id'] for r in batch}
            scopes = [{'task_id': task['task_id'], 'target_ids': review_scope(task)} for task in planned
                      if task['task_id'] in identities and review_scope(task)]
            return {'grounding_policy': AUTHORING_GROUNDING_POLICY, 'units': batch,
                    **({'review_request': {'reason': review_request['reason'], 'targets': scopes}} if scopes else {}),
                    'previous_findings': [{**{k: v for k, v in f.items() if k != 'content_ids'},
                                           'task_ids': [identity for identity in f['task_ids'] if identity in identities],
                                           'partial_context': bool(set(f['task_ids']) - identities),
                                           'original_content_ids': f.get('content_ids', [])}
                        for f in previous if set(f['task_ids']) & identities]}
        for batch in _batches(records, budget, 'authoring_review', review_payload):
            ids = {entry['task_id'] for entry in batch}
            def validate_review(raw):
                value = _parse(AuthoringReviewResponse, raw)
                for finding in value['findings']:
                    if set(finding['task_ids']) - ids:
                        raise ModelContractError([{'path': ['findings'], 'code': 'review_unknown_task',
                                                  'detail': 'Select only tasks supplied in this review batch.'}])
                    if _content_rejected(finding) and not finding['task_ids']:
                        raise ModelContractError([{'path': ['findings'], 'code': 'review_unlocated_content',
                                                  'detail': 'Identify the supplied task whose content is rejected.'}])
                    for identity in finding['task_ids']:
                        record = next(r for r in batch if r['task_id'] == identity)
                        content_ids = {entry['content_id'] for key in ('blocks', 'cell_fills', 'row_fills', 'attachments')
                                       for entry in record['content'].get(key, [])}
                        if set(finding['content_ids']) - content_ids:
                            raise ModelContractError([{'path': ['findings'], 'code': 'review_unknown_content',
                                                      'detail': 'Select content references in the identified task.'}])
                normalized = []
                for finding in value['findings']:
                    if _content_rejected(finding):
                        normalized.extend({**finding, 'task_ids': [identity]} for identity in finding['task_ids'])
                    else:
                        normalized.append(finding)
                value['findings'] = normalized
                return value
            try:
                value = await _ask(execute, budget, 'authoring_review', review_payload(batch), validate_review)
                findings.extend(value['findings'])
                completed.update(ids)
                replaced.update(i for i, f in enumerate(previous) if f['task_ids'] and set(f['task_ids']) <= ids)
            except _LOCAL_FAILURES as error:
                findings.append({'code': 'review_unavailable', 'detail': f'本批实际正文尚未复核（{type(error).__name__}）。',
                                 'task_ids': sorted(ids), 'suggestion': '保留文件，后续针对这些单元重试复核。'})
        return findings, completed, replaced
    findings, _, _ = await review(review_records)
    initial_review_ref = artifacts.put_json(AuthoringReviewResponse(findings=findings).model_dump())
    from docreader.tender.authoring_revision import revise_units
    def revision_payload(task):
        payload, sources, _ = _load_task(artifacts, context, task, basis, materials)
        if review_scope(task):
            payload['review_request'] = {'reason': review_request['reason'], 'target_ids': review_scope(task)}
        return payload, sources, _support_bindings(sources, _task_materials(materials, task))
    def revision_bind(task, raw):
        payload, sources, source_issues = _load_task(artifacts, context, task, basis, materials)
        result = bind_writing_response(task, raw, sources, _task_materials(materials, task),
                                      support_views=payload['supports'])
        result['gaps'].extend(source_issues)
        return result
    superseded_refs = []
    safe_units = _adopt_reviewed(units, findings, artifacts, superseded_refs)
    revised, revision_refs, revision_gaps = await revise_units(
        artifacts, budget, execute, planned, units, findings, revision_payload, revision_bind)
    superseded_refs.extend(revision_refs)
    changed = {new['task_id'] for old, new in zip(units, revised) if old != new}
    if changed:
        affected = changed | {identity for f in findings if set(f['task_ids']) & changed for identity in f['task_ids']}
        records = []
        for task, unit in zip(planned, revised):
            if task['task_id'] in affected:
                payload, sources, _ = await asyncio.to_thread(_load_task, artifacts, context, task, basis, materials)
                record = _review_record(task, unit, payload, sources, _task_materials(materials, task))
                if record is not None:
                    records.append(record)
        new_findings, completed, replaced = await review(records, findings)
        adopted = []
        for old, new, safe in zip(units, revised, safe_units):
            identity = old['task_id']
            if identity in changed:
                if identity in completed:
                    adopted.extend(_adopt_reviewed([new], new_findings, artifacts, superseded_refs))
                else:
                    superseded_refs.append(artifacts.put_json(new))
                    adopted.append(safe)
            elif identity in completed:
                adopted.extend(_adopt_reviewed([old], [*findings, *new_findings], artifacts, superseded_refs))
            else:
                adopted.append(safe)
        safe_units = adopted
        findings = [f for i, f in enumerate(findings) if i not in replaced] + new_findings
    gaps.extend(revision_gaps)
    units = safe_units
    findings.extend(structure_results['findings'])
    superseded_refs = list({_json(ref): ref for ref in superseded_refs}.values())
    unit_refs = [artifacts.put_json(unit) for unit in units]
    for task, unit in zip(planned, units):
        gaps.extend(_gap('writing_gap', detail, task) for detail in unit['gaps'] if detail.strip())
    review_ref = artifacts.put_json(AuthoringReviewResponse(findings=findings).model_dump())
    by_id = {t['task_id']: t for t in planned}
    for finding in findings:
        for task_id in finding['task_ids'] or [None]:
            gaps.append(_gap(finding['code'], finding['detail'] + (' 建议：' + finding['suggestion'] if finding['suggestion'] else ''),
                             by_id.get(task_id)))
    gaps.extend({'code': i['code'], 'detail': i['detail'],
                 **({'target_id': i['target_id']} if i.get('target_id') else {})} for i in outline['issues'])
    from docreader.tender.authoring_export import export_authoring_document
    exported = await asyncio.to_thread(export_authoring_document, artifacts, outline, units, gaps,
                                      requirements=requirements)
    all_gaps = list({_json(g): g for g in [*gaps, *exported['gaps']]}.values())
    result = AuthoringDocument(authoring_input=snapshot, outline_ref=outline_ref, requirements_ref=requirements_ref,
                               basis_ref=basis_ref,
                               unit_refs=unit_refs, review_ref=review_ref, initial_review_ref=initial_review_ref,
                               superseded_unit_refs=superseded_refs, docx_ref=exported['docx_ref'],
                               package_ref=exported['package_ref'], gaps=all_gaps, usage=AuthoringUsage()).model_dump()
    return artifacts.put_json(result)
