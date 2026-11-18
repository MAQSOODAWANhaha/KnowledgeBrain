"""Choose among supplied directory source candidates without inventing new content."""

from copy import deepcopy
import hashlib
import json

from pydantic import ValidationError

from docreader.tender.directory_compile import (
    DirectoryAdjudication, DirectoryPlanRejected, _Source, _attach_template_parts, _root,
    _comparison_body, _segment_template_content, _validate_body_owners, _window, _window_text,
)


def _candidate(chapter, parents):
    if chapter is None:
        return None
    value = deepcopy(chapter)
    parent = next((p for p in parents if p['id'] == chapter.get('parent_id')), None)
    value['parent_heading'] = deepcopy(parent['heading']) if parent else None
    return value


def source_case_issue(issue, kind, current, alternative=None, parents=(), alternative_parents=()):
    """Attach immutable source candidates; identities never depend on diagnostic prose."""
    case = {'kind': kind, 'current': _candidate(current, parents),
            'alternative': _candidate(alternative, alternative_parents)}
    identity = {'kind': kind}
    for option in ('current', 'alternative'):
        value = case[option]
        identity[option] = None if value is None else {key: value.get(key) for key in (
            'heading', 'entry', 'parent_heading', 'body_windows', 'text', 'response_kind', 'subjects',
            'presentation', 'group', 'group_heading', 'condition', 'condition_windows', 'obligations', 'clause_windows')}
        if value is not None and value.get('parent_id') is not None and value['parent_heading'] is None:
            identity[option]['unadopted_parent'] = value['parent_id']
    encoded = json.dumps(identity, ensure_ascii=False, sort_keys=True, separators=(',', ':'))
    case['case_id'] = 'directory-' + hashlib.sha256(encoded.encode()).hexdigest()
    return {**issue, 'directory_case_id': case['case_id'], '_directory_case': case}


def _pending_cases(compiled, source):
    cases = {}
    for issue in compiled['issues']:
        case = issue.get('_directory_case')
        if case is None:
            continue
        if issue.get('directory_case_id') != case['case_id']:
            raise DirectoryPlanRejected('directory case identity changed')
        for option in ('current', 'alternative'):
            value = case[option]
            if value is None:
                continue
            heading = value['heading']
            source.line(heading['document_id'], heading['page'], heading['line'])
            refs = value.get('lines', [])
            text = '\n'.join(source.line(ref['document_id'], ref['page'], ref['line']) for ref in refs)
            if refs and text.strip() != value.get('text', '').strip():
                raise DirectoryPlanRejected('directory candidate source changed')
            for raw in [*value.get('condition_windows', []), *value.get('clause_windows', [])]:
                _window(raw, source, 'directory candidate source')
            conditions = value.get('condition_windows', [])
            if conditions and '\n'.join(_window_text(source, _window(w, source, 'directory condition'))
                                       for w in conditions).strip() != (value.get('condition') or '').strip():
                raise DirectoryPlanRejected('directory condition source changed')
        if case['case_id'] in cases and cases[case['case_id']] != case:
            raise DirectoryPlanRejected('directory case candidates disagree')
        cases[case['case_id']] = case
    return cases


def directory_cases(compiled, documents):
    """Finite source options only; full text is already in the material reading."""
    source = _Source(documents)
    cases = _pending_cases(compiled, source)
    fields = ('id', 'heading', 'entry', 'parent_id', 'parent_heading', 'response_kind', 'subjects',
              'presentation', 'group', 'group_heading', 'condition_windows', 'clause_windows')
    return [{'case_id': case['case_id'], 'kind': case['kind'], **{
        option: None if case[option] is None else {
            **{key: deepcopy(case[option].get(key)) for key in fields},
            'body_windows': _windows({(r['document_id'], r['page'], r['line'])
                                      for r in case[option].get('lines', [])}, source)}
        for option in ('current', 'alternative')}} for case in cases.values()]


def _body_positions(candidate):
    return {(ref['document_id'], ref['page'], ref['line']) for ref in candidate.get('lines', [])}


def _positions(candidate, *, context=True):
    positions = _body_positions(candidate)
    heading = candidate['heading']
    positions.add((heading['document_id'], heading['page'], heading['line']))
    if context:
        positions.update((ref['document_id'], ref['page'], ref['line'])
                         for ref in candidate.get('obligation_lines', []))
        for raw in [*candidate.get('condition_windows', []), *candidate.get('clause_windows', [])]:
            positions.update((raw['document_id'], raw['page'], line)
                             for line in range(raw['start_line'], raw['end_line'] + 1))
    return positions


def _windows(positions, source=None):
    windows = []
    for document_id, page, line in sorted(positions, key=(lambda point: source.position(*point)) if source else None):
        if windows and (windows[-1]['document_id'], windows[-1]['page'], windows[-1]['end_line'] + 1) == (
                document_id, page, line):
            windows[-1]['end_line'] = line
        else:
            windows.append({'document_id': document_id, 'page': page, 'start_line': line, 'end_line': line})
    return windows


def _source_record(candidate, source, action):
    windows = _windows(_positions(candidate), source)
    return {'heading': deepcopy(candidate['heading']), 'source_windows': windows,
            'original_text': '\n'.join(_window_text(source, _window(raw, source, 'directory record')) for raw in windows),
            'response_kind': candidate.get('response_kind', 'form'), 'subjects': deepcopy(candidate.get('subjects', [])),
            'presentation': candidate['presentation'], 'action': action, 'enterprise_confirmed': False}


def _window_positions(raw):
    return {(raw['document_id'], raw['page'], line) for line in range(raw['start_line'], raw['end_line'] + 1)}


def reference_exclusions(compiled):
    """Finite body-only source references; reference headings and original text stay in provenance."""
    exclusions = []
    for decision in compiled.get('directory_decisions', []):
        if decision['action'] != 'not_applicable_reference':
            continue
        body = _body_positions(decision['selected_candidate'])
        stored = {point for record in decision['source_records'] for raw in record['source_windows']
                  for point in _window_positions(raw)}
        if not body <= stored:
            raise DirectoryPlanRejected('not_applicable_reference body has incomplete stored source provenance')
        exclusions.append({'case_id': decision['case_id'], 'body_windows': _windows(body),
                           'basis_windows': deepcopy(decision['basis_windows']), 'basis_text': decision['basis_text']})
    return exclusions


def validate_reference_exclusions(compiled):
    """A reference-only decision cannot coexist with the same body coordinates in a submitted chapter."""
    for exclusion in reference_exclusions(compiled):
        excluded = {point for raw in exclusion['body_windows'] for point in _window_positions(raw)}
        for chapter in compiled['chapters']:
            if chapter.get('unplaced'):
                continue
            overlap = excluded & _body_positions(chapter)
            if overlap:
                document_id, page, line = min(overlap)
                raise DirectoryPlanRejected(
                    f'not_applicable_reference case {exclusion["case_id"]} body remains in chapter {chapter["id"]} '
                    f'at {document_id} page {page} line {line}; select another response source or keep the case unresolved')


def validate_requirement_exclusions(compiled, assignments):
    """Keep excluded reference bodies out of chapter assignments; guidance scopes remain available."""
    exclusions = reference_exclusions(compiled)
    for assignment in assignments:
        if assignment.get('scope') not in {'chapter', 'chapters'}:
            continue
        selected = _window_positions(assignment)
        for raw in assignment.get('continuation_windows', []):
            selected.update(_window_positions(raw))
        for exclusion in exclusions:
            excluded = {point for raw in exclusion['body_windows'] for point in _window_positions(raw)}
            overlap = selected & excluded
            if overlap:
                document_id, page, line = min(overlap)
                raise DirectoryPlanRejected(
                    f'chapter requirement reintroduces not_applicable_reference case {exclusion["case_id"]} body '
                    f'at {document_id} page {page} line {line}; use a guidance scope or reconsider the source decision')


def _parsed_decisions(compiled, source, decisions):
    cases = _pending_cases(compiled, source)
    if not isinstance(decisions, list):
        raise DirectoryPlanRejected('directory adjudications must be a list')
    known = {chapter['id'] for chapter in compiled['chapters'] if not chapter.get('unplaced')}
    selected, values = set(), []
    for raw in decisions:
        try:
            decision = DirectoryAdjudication.model_validate(raw).model_dump()
        except ValidationError as error:
            raise DirectoryPlanRejected('invalid directory adjudication: ' + str(error)) from None
        identity, action, ids = decision['case_id'], decision['action'], decision['chapter_ids']
        if identity not in cases:
            raise DirectoryPlanRejected('directory adjudication contains an unknown case')
        if identity in selected:
            raise DirectoryPlanRejected('directory case must be adjudicated exactly once')
        if len(ids) != len(set(ids)) or any(identity not in known for identity in ids):
            raise DirectoryPlanRejected('directory adjudication contains an invalid chapter')
        basis = [_window(raw, source, 'directory adjudication basis') for raw in decision['basis_windows']]
        if action != 'unresolved' and (not basis or not all(_window_text(source, w).strip() for w in basis)):
            raise DirectoryPlanRejected('directory selection needs a nonempty source basis')
        case = cases[identity]
        if action == 'conditional' and (case['kind'] != 'applicability'
                                         or not case['current'].get('condition_windows')):
            raise DirectoryPlanRejected('conditional needs a supplied explicit source condition')
        if action == 'not_applicable_reference' and case['kind'] != 'extra':
            raise DirectoryPlanRejected('not_applicable_reference needs a separate extra source candidate')
        if action in {'alternative', 'subjects_alternative', 'reference', 'container', 'not_applicable_reference'} and case['alternative'] is None:
            raise DirectoryPlanRejected('directory action needs a supplied alternative candidate')
        if action == 'subjects_alternative':
            current, alternative = case['current'], case['alternative']
            if (case['kind'] != 'conflict' or any(current.get(key) != alternative.get(key)
                                                for key in ('id', 'heading', 'entry'))
                    or _comparison_body(current, source) != _comparison_body(alternative, source)):
                raise DirectoryPlanRejected('directory subject choice needs the same source material')
            if sorted(subject['role'] for subject in current.get('subjects', [])) == sorted(
                    subject['role'] for subject in alternative.get('subjects', [])):
                raise DirectoryPlanRejected('directory subject choice needs a different subject layout')
        if action in {'reference', 'container'} and not ids:
            raise DirectoryPlanRejected('directory reference/container needs explicit chapter targets')
        if action not in {'reference', 'container', 'not_applicable_reference'} and ids not in ([], [case['current']['id']]):
            raise DirectoryPlanRejected('directory selection targets do not match the supplied candidate')
        selected.add(identity)
        values.append((case, decision, basis))
    if selected != set(cases):
        raise DirectoryPlanRejected('directory adjudications must cover every pending case exactly once')
    return values


def _parent(candidate, chapters):
    heading = candidate.get('parent_heading')
    if heading is None:
        if candidate.get('parent_id') is not None:
            raise DirectoryPlanRejected('directory candidate parent is not an adopted source option')
        return None
    parents = [chapter['id'] for chapter in chapters if chapter['heading'] == heading]
    if len(parents) != 1:
        raise DirectoryPlanRejected('directory candidate parent does not map to one selected source heading')
    return parents[0]


def _compatible(candidate, chapter):
    return ((candidate.get('response_kind', 'form'), candidate.get('subjects', [])) == (
        chapter.get('response_kind', 'form'), chapter.get('subjects', []))
        and (candidate.get('entry') is None or chapter.get('entry') is None
             or candidate['entry'] == chapter['entry']))


def _pure_parent(candidate):
    return (candidate.get('presentation', 'section') == 'section' and candidate.get('entry') is None
            and not any(candidate.get(field) for field in (
                'subjects', 'text', 'lines', 'body_windows', 'template_parts', 'template_segments',
                'condition', 'condition_windows', 'obligations', 'obligation_lines', 'clause_windows',
                'requirements', 'materials', 'material_segments', 'parameter_items')))


def _apply_choices(compiled, documents, decisions):
    source = _Source(documents)
    values = _parsed_decisions(compiled, source, decisions)
    work = deepcopy(compiled)
    chapters = work['chapters']
    selected, replaced, contained = {}, {}, set()
    # Select source choices before checking references against the resulting Word carriers.
    for case, decision, _ in values:
        if decision['action'] not in {'alternative', 'subjects_alternative'}:
            continue
        current = next((c for c in chapters if c['id'] == case['current']['id']), None)
        if current is None:
            raise DirectoryPlanRejected('directory current candidate is no longer selected')
        if decision['action'] == 'subjects_alternative':
            candidate = deepcopy(current)
            candidate['subjects'] = deepcopy(case['alternative'].get('subjects', []))
        else:
            candidate = deepcopy(case['alternative'])
            if candidate.get('lines') == current.get('lines') and candidate.get('text') == current.get('text'):
                for field in ('template_parts', 'template_segments'):
                    if not candidate.get(field) and current.get(field):
                        candidate[field] = deepcopy(current[field])
            if not candidate.get('template_parts'):
                _attach_template_parts([candidate], source, work['issues'])
            if not candidate.get('template_segments'):
                _segment_template_content([candidate], source)
        subordinate = case['kind'] == 'extra' and candidate.get('parent_heading') is not None
        if subordinate:
            candidate['id'] = 'directory-choice-' + case['case_id'].removeprefix('directory-')
            if any(c['id'] == candidate['id'] for c in chapters):
                raise DirectoryPlanRejected('directory alternative already exists')
            chapters.append(candidate)
        else:
            candidate['id'] = current['id']
            fingerprint = {key: value for key, value in candidate.items() if key not in {'parent_id', 'id'}}
            if current['id'] in replaced and replaced[current['id']] != fingerprint:
                raise DirectoryPlanRejected('directory alternatives disagree on one selected chapter')
            replaced[current['id']] = fingerprint
            chapters[chapters.index(current)] = candidate
        selected[case['case_id']] = candidate
    for case, decision, _ in values:
        if decision['action'] == 'alternative':
            candidate = selected[case['case_id']]
            candidate['parent_id'] = _parent(candidate, chapters)
    for case, decision, _ in values:
        if decision['action'] != 'container':
            continue
        candidate = deepcopy(case['alternative'])
        children = [c for c in chapters if c['id'] in decision['chapter_ids']]
        if contained.intersection(decision['chapter_ids']):
            raise DirectoryPlanRejected('directory child cannot be claimed by two containers')
        pure_parent = _pure_parent(candidate)
        if any((not pure_parent and not _compatible(candidate, child))
               or candidate['heading'] == child['heading'] for child in children):
            raise DirectoryPlanRejected('directory container cannot erase a response subject or its own heading')
        body = _positions(candidate) - {
            (candidate['heading']['document_id'], candidate['heading']['page'], candidate['heading']['line'])}
        covered = set().union(*(_positions(child) for child in children))
        if not body <= covered:
            raise DirectoryPlanRejected('directory container has source content not carried by its selected children')
        candidate['parent_id'] = _parent(candidate, chapters)
        existing = next((c for c in chapters if pure_parent and _pure_parent(c)
                         and c['heading'] == candidate['heading']), None)
        candidate['id'] = existing['id'] if existing else 'directory-container-' + case['case_id'].removeprefix('directory-')
        candidate.update(text='', lines=[], body_windows=[], obligations='', obligation_lines=[],
                         clause_windows=[], condition=None, condition_windows=[])
        for field in ('template_parts', 'template_segments'):
            candidate.pop(field, None)
        for child in children:
            child['parent_id'] = candidate['id']
        if existing is None:
            chapters.append(candidate)
        else:
            chapters[chapters.index(existing)] = candidate
        contained.update(decision['chapter_ids'])
        selected[case['case_id']] = candidate
    records, resolved = [], set()
    statuses = {'current': '来源取舍已决定；不代表企业满足或已履行。',
                'alternative': '已选择另一来源方案；不代表企业满足或已履行。',
                'subjects_alternative': '主体布局采用另一来源方案，其余正文及条件采用当前方案；不代表企业满足或已履行。',
                'reference': '完整来源已由所选响应承载；保留来源对应。',
                'not_applicable_reference': '本次不作为提交响应；完整范本仅供参考，未履行。',
                'container': '保留来源父标题；正文由所选子项完整承载。',
                'conditional': '源条件明确；企业是否满足或采用未确认。',
                'unresolved': '来源取舍未决；保留原待核事项。'}
    for case, decision, basis in values:
        action = decision['action']
        current = next((c for c in chapters if c['id'] == case['current']['id']), None)
        candidate = selected.get(case['case_id'], current)
        if action == 'subjects_alternative':
            original = next((c for c in compiled['chapters'] if c['id'] == case['current']['id']), None)
            if (current is None or original is None
                    or {key: value for key, value in current.items() if key != 'subjects'} != {
                        key: value for key, value in original.items() if key != 'subjects'}
                    or current.get('subjects', []) != case['alternative'].get('subjects', [])):
                raise DirectoryPlanRejected('directory subject choice conflicts with another selected disposition')
        if action == 'current':
            fields = ('condition', 'condition_windows') if case['kind'] == 'applicability' else (
                'heading', 'entry', 'body_windows', 'response_kind', 'subjects', 'presentation', 'group', 'group_heading',
                'condition', 'condition_windows', 'obligations', 'obligation_lines', 'clause_windows')
            if current is None or any(current.get(field) != case['current'].get(field) for field in fields):
                raise DirectoryPlanRejected('directory current conflicts with another selected disposition')
            if current.get('parent_id') and not any(c['id'] == current['parent_id'] for c in chapters):
                raise DirectoryPlanRejected('directory current has an unadopted parent')
        if action in {'reference', 'not_applicable_reference'}:
            candidate = case['alternative']
            if action == 'reference':
                carriers = [c for c in chapters if c['id'] in decision['chapter_ids']]
                if any(not _compatible(candidate, carrier) for carrier in carriers):
                    raise DirectoryPlanRejected('directory reference cannot mix response roles or subjects (including submission entries)')
                covered = set().union(*(_positions(carrier) for carrier in carriers))
                if not _positions(candidate) <= covered:
                    raise DirectoryPlanRejected('directory reference has source content not covered by the selected Word carriers')
        if action == 'conditional' and (current is None or current.get('condition') != case['current'].get('condition')):
            raise DirectoryPlanRejected('directory condition changed in another disposition')
        originals = ([case['current'], case['alternative']] if action == 'unresolved'
                     else [case['alternative']] if action == 'container' else [candidate])
        record = {**deepcopy(decision), 'kind': case['kind'], 'status': statuses[action], 'enterprise_confirmed': False,
                  'basis_text': '\n'.join(_window_text(source, raw) for raw in basis),
                  'candidates': {option: deepcopy(case[option]) for option in ('current', 'alternative')},
                  'selected_candidate': deepcopy(candidate),
                  'source_records': [_source_record(value, source, action) for value in originals if value is not None]}
        if action == 'subjects_alternative':
            record['field_sources'] = {'subjects': 'alternative', 'other_fields': 'current'}
        records.append(record)
        if action != 'unresolved':
            resolved.add(case['case_id'])
    _validate_body_owners(chapters)
    parents = {chapter['id']: chapter.get('parent_id') for chapter in chapters}
    for identity in parents:
        _root(identity, parents)
    work['issues'] = [issue for issue in work['issues'] if issue.get('directory_case_id') not in resolved]
    work.setdefault('directory_decisions', []).extend(records)
    validate_reference_exclusions(work)
    return work


def validate_directory_adjudications(compiled, documents, decisions):
    """Simulate the complete batch so structural rejection is returned to the same model request."""
    _apply_choices(compiled, documents, decisions)
    return decisions


def apply_directory_adjudications(compiled, documents, decisions):
    """Apply atomically; unresolved cases retain their original source issues."""
    work = _apply_choices(compiled, documents, decisions)
    compiled.clear()
    compiled.update(work)
    return compiled


def decision_issues(compiled):
    """Preserve resolved source choices in the public outline and its submission guide."""
    def locations(windows):
        return '；'.join(f"{raw['document_id']} 第{raw['page'] + 1}页 "
                        f"第{raw['start_line'] + 1}–{raw['end_line'] + 1}行" for raw in windows)

    issues = []
    for decision in compiled.get('directory_decisions', []):
        action = decision['action']
        if action == 'unresolved':
            continue
        lines = [f"来源事项：{decision['case_id']}", f"来源动作：{action}", decision['status'],
                 '依据位置：' + locations(decision['basis_windows']), '依据原文：', decision['basis_text']]
        if decision.get('chapter_ids'):
            lines.append('对应来源章节：' + '、'.join(decision['chapter_ids']))
        for record in decision['source_records']:
            heading = record['heading']
            lines.extend([f"来源标题：{heading['document_id']} 第{heading['page'] + 1}页 第{heading['line'] + 1}行",
                          '完整来源位置：' + locations(record['source_windows']),
                          '完整来源原文：', record['original_text']])
            if record.get('subjects'):
                lines.append('提交主体：' + '、'.join(subject['role'] for subject in record['subjects']))
        issues.append({'code': 'outline_directory_source_decision', 'detail': '\n'.join(lines), 'rule_refs': []})
        if action == 'conditional':
            issues.append({'code': 'outline_directory_conditional_requirement', 'rule_refs': [],
                           'detail': '企业身份和适用情况未确认；请按真实身份选择并填写，不代表已经符合。\n'
                                     + '\n'.join(lines)})
    return issues
