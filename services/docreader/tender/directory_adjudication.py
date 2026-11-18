"""Apply model dispositions to host-owned pending requirement source cases."""

from copy import deepcopy
import hashlib
import json

from pydantic import ValidationError

from docreader.tender.directory_compile import (
    DirectoryPlanRejected, RequirementAdjudication, _Source, _record_requirement_case, _window, _window_text,
    apply_requirements,
)


def _pending_cases(compiled, source):
    active_ids = {issue.get('requirement_case_id') for issue in compiled['issues']
                  if issue['code'] == 'outline_directory_requirement_unassigned'}
    cases = {}
    for case in compiled.get('_requirement_cases', []):
        if case['id'] not in active_ids:
            continue
        windows = [_window(window, source, 'requirement case source') for window in case['source_windows']]
        if '\n'.join(_window_text(source, window) for window in windows).strip() != case['source_text']:
            raise DirectoryPlanRejected('requirement case source changed')
        cases[case['id']] = case
    return cases


def requirement_cases(compiled, documents):
    """Known IDs and coordinates only; the classification request already has the full reading."""
    cases = _pending_cases(compiled, _Source(documents))
    return [{'id': case['id'], 'source_windows': deepcopy(case['source_windows'])} for case in cases.values()]


def _combined_windows(cases, source):
    positions = set()
    for case in cases:
        for raw in case['source_windows']:
            document_id, page, start, end = _window(raw, source, 'requirement case source')
            positions.update((document_id, page, line) for line in range(start, end + 1))
    if len({document_id for document_id, _, _ in positions}) != 1:
        raise DirectoryPlanRejected('merged requirement cases must belong to the same document')
    windows = []
    for document_id, page, line in sorted(positions):
        if windows and windows[-1][:2] == (document_id, page) and windows[-1][3] + 1 == line:
            windows[-1] = (*windows[-1][:3], line)
        else:
            windows.append((document_id, page, line, line))
    return windows


def _validated_decisions(compiled, documents, decisions):
    source = _Source(documents)
    cases = _pending_cases(compiled, source)
    if not isinstance(decisions, list):
        raise DirectoryPlanRejected('requirement adjudications must be a list')
    known = {chapter['id'] for chapter in compiled['chapters'] if not chapter.get('unplaced')}
    covers = {chapter['id'] for chapter in compiled['chapters'] if chapter.get('presentation') == 'cover'}
    selected, validated = set(), []
    for raw in decisions:
        try:
            decision = RequirementAdjudication.model_validate(raw).model_dump()
        except ValidationError as error:
            raise DirectoryPlanRejected('invalid requirement adjudication: ' + str(error)) from None
        item_ids, chapter_ids, scope = decision['item_ids'], decision['chapter_ids'], decision['scope']
        if not item_ids or len(item_ids) != len(set(item_ids)) or selected.intersection(item_ids):
            raise DirectoryPlanRejected('requirement case must be adjudicated exactly once')
        if any(identity not in cases for identity in item_ids):
            raise DirectoryPlanRejected('requirement adjudication contains an unknown case')
        if len(chapter_ids) != len(set(chapter_ids)) or any(identity not in known for identity in chapter_ids):
            raise DirectoryPlanRejected('requirement adjudication contains an invalid chapter')
        if ((scope == 'chapter' and len(chapter_ids) != 1)
                or (scope == 'chapters' and not chapter_ids)
                or (scope not in {'chapter', 'chapters'} and chapter_ids)):
            raise DirectoryPlanRejected('requirement adjudication chapter targets do not match its scope')
        if scope in {'chapter', 'chapters'} and covers.intersection(chapter_ids):
            raise DirectoryPlanRejected('cover is not a chapter response target; use document for whole-document guidance')
        basis = [_window(window, source, 'requirement adjudication basis') for window in decision['basis_windows']]
        if scope == 'not_applicable' and (not basis or not all(_window_text(source, window).strip() for window in basis)):
            raise DirectoryPlanRejected('not_applicable requirement needs a nonempty source basis')
        windows = _combined_windows([cases[identity] for identity in item_ids], source)
        from docreader.tender.directory_choices import validate_requirement_exclusions
        validate_requirement_exclusions(compiled, [
            {**dict(zip(('document_id', 'page', 'start_line', 'end_line'), window)), 'scope': scope}
            for window in windows])
        selected.update(item_ids)
        validated.append((decision, windows, basis))
    if selected != set(cases):
        raise DirectoryPlanRejected('requirement adjudications must cover every pending case exactly once')
    return source, validated


def validate_requirement_adjudications(compiled, documents, decisions):
    """Validate source/identity coverage, without substituting keyword rules for model judgment."""
    _validated_decisions(compiled, documents, decisions)


def _record_decision(compiled, source, decision, windows, basis, result_case_ids=()):
    cases = {case['id']: case for case in compiled.get('_requirement_cases', [])}
    record = {'item_ids': sorted(decision['item_ids']),
              'merged_from_case_ids': sorted({identity for case_id in decision['item_ids']
                  for identity in cases[case_id].get('merged_from_case_ids', [])}),
              'result_case_ids': sorted(result_case_ids), 'scope': str(decision['scope']),
              'chapter_ids': sorted(decision['chapter_ids']),
              'source_windows': [dict(zip(('document_id', 'page', 'start_line', 'end_line'), window))
                                 for window in windows],
              'source_text': '\n'.join(_window_text(source, window) for window in windows).strip(),
              'basis_windows': [dict(zip(('document_id', 'page', 'start_line', 'end_line'), window))
                                for window in basis],
              'basis_text': '\n'.join(_window_text(source, window) for window in basis).strip(),
              'enterprise_confirmed': False}
    identity = json.dumps(record, ensure_ascii=False, sort_keys=True, separators=(',', ':'))
    record['decision_id'] = 'requirement-decision-' + hashlib.sha256(identity.encode('utf-8')).hexdigest()
    records = compiled.setdefault('requirement_decisions', [])
    if not any(existing['decision_id'] == record['decision_id'] for existing in records):
        records.append(record)


def requirement_decision_issues(compiled):
    """Expose source decisions in the outline/guide without asserting enterprise fulfillment."""
    def locations(windows):
        return '；'.join(f"{raw['document_id']} 第{raw['page'] + 1}页 "
                        f"第{raw['start_line'] + 1}–{raw['end_line'] + 1}行" for raw in windows)

    issues = []
    for record in compiled.get('requirement_decisions', []):
        lines = ['来源裁定记录：' + record['decision_id'],
                 '来源要求事项：' + '、'.join(record['item_ids']),
                 '来源出口(scope)：' + record['scope'],
                 '企业事实未确认；来源处置不代表已经符合或已执行。']
        if record['merged_from_case_ids']:
            lines.append('合并前来源事项：' + '、'.join(record['merged_from_case_ids']))
        if record['result_case_ids']:
            lines.append('保留未决来源事项：' + '、'.join(record['result_case_ids']))
        if record['chapter_ids']:
            lines.append('对应来源章节(chapter_ids)：' + '、'.join(record['chapter_ids']))
        lines.extend(['完整来源位置：' + locations(record['source_windows']),
                      '完整来源原文：', record['source_text'],
                      '依据位置：' + (locations(record['basis_windows']) or '本项未选择额外依据窗口'),
                      '依据原文：', record['basis_text']])
        issues.append({'code': 'outline_directory_source_decision', 'detail': '\n'.join(lines), 'rule_refs': []})
    return issues


def apply_requirement_adjudications(compiled, documents, decisions):
    """Only remove the original issues explicitly resolved by a validated disposition."""
    source, validated = _validated_decisions(compiled, documents, decisions)
    resolved = {identity for decision, _, _ in validated
                if decision['scope'] != 'unassigned' or len(decision['item_ids']) > 1
                for identity in decision['item_ids']}
    compiled['issues'] = [issue for issue in compiled['issues']
                          if not (issue['code'] == 'outline_directory_requirement_unassigned'
                                  and issue.get('requirement_case_id') in resolved)]
    for decision, windows, basis in validated:
        scope = decision['scope']
        if scope == 'unassigned':
            if len(decision['item_ids']) > 1:
                raw_windows = [dict(zip(('document_id', 'page', 'start_line', 'end_line'), window)) for window in windows]
                raw = {**raw_windows[0], 'continuation_windows': raw_windows[1:], 'scope': scope}
                issue = {'code': 'outline_directory_requirement_unassigned',
                         'detail': '\n'.join(_window_text(source, window) for window in windows).strip()}
                case = _record_requirement_case(compiled, source, raw, windows, issue)
                case['merged_from_case_ids'] = sorted(set(
                    [*case.get('merged_from_case_ids', []), *decision['item_ids']]) - {case['id']})
                compiled['issues'].append(issue)
                result_case_ids = [case['id']]
            else:
                result_case_ids = decision['item_ids']
                for issue in compiled['issues']:
                    if (issue['code'] == 'outline_directory_requirement_unassigned'
                            and issue.get('requirement_case_id') in decision['item_ids']):
                        issue['detail'] = '\n'.join(_window_text(source, window) for window in windows).strip()
            _record_decision(compiled, source, decision, windows, basis, result_case_ids)
            continue
        if scope in {'chapter', 'chapters', 'document'}:
            raw_windows = [dict(zip(('document_id', 'page', 'start_line', 'end_line'), window)) for window in windows]
            apply_requirements(compiled, documents, [{**raw_windows[0], 'continuation_windows': raw_windows[1:],
                                                     'scope': scope, 'chapter_ids': decision['chapter_ids']}],
                               complete_basis=True)
            _record_decision(compiled, source, decision, windows, basis)
            continue
        text = '\n'.join(_window_text(source, window) for window in windows).strip()
        if scope == 'procedure':
            code, detail = 'outline_directory_procedure_requirement', '待执行（不代表已完成）：\n' + text
        else:
            code = 'outline_directory_requirement_not_applicable'
            detail = '本次不适用；保留原条款：\n' + text + '\n依据：\n' + '\n'.join(
                _window_text(source, window) for window in basis).strip()
        compiled['issues'].append({'code': code, 'detail': detail})
        _record_decision(compiled, source, decision, windows, basis)
    return compiled
