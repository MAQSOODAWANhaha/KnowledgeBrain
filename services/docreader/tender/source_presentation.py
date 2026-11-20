"""Apply explicit source decorations to final source-bound prose, never the reading."""
from copy import deepcopy
import hashlib
import json
import re
from typing import Literal

from pydantic import Field, field_validator

from docreader.integrations.artifacts import ArtifactIntegrityError
from docreader.tender.directory_compile import DirectoryPoint, DirectoryWindow
from docreader.tender.schema import ArtifactRef, Contract, Digest, Identity, NonnegativeInt, Text


class OriginalCharacters(Contract):
    start: NonnegativeInt
    end: NonnegativeInt
    text_sha256: Digest


class LayoutFragment(OriginalCharacters):
    page: NonnegativeInt
    line: NonnegativeInt
    original_chars: OriginalCharacters


class ContextApproval(Contract):
    owner_heading: DirectoryPoint
    context_kind: Literal['clause', 'condition']
    context_window: DirectoryWindow
    context_raw_rows_sha256: Digest
    review_evidence_ref: ArtifactRef
    enterprise_confirmed: Literal[False]

    @field_validator('enterprise_confirmed', mode='before')
    @classmethod
    def explicit_false(cls, value):
        if value is not False:
            raise ValueError('source context review cannot confirm enterprise facts')
        return value


class SourceDecorationFact(Contract):
    kind: Literal['source_decoration']
    label: Text
    document_id: Identity
    original_ref: ArtifactRef
    role: Literal['page_header', 'page_footer']
    layout_fragments: list[LayoutFragment] = Field(min_length=1)
    review_evidence_ref: ArtifactRef
    enterprise_confirmed: Literal[False]
    supersedes: list[Identity] = Field(default_factory=list)
    context_approvals: list[ContextApproval] = Field(default_factory=list, exclude_if=lambda value: not value)

    @field_validator('enterprise_confirmed', mode='before')
    @classmethod
    def explicit_false(cls, value):
        if value is not False:
            raise ValueError('source decoration review cannot confirm enterprise facts')
        return value


def _sha(text):
    return hashlib.sha256(text.encode()).hexdigest()


def _rows_sha(rows):
    return _sha(json.dumps(rows, ensure_ascii=False, sort_keys=True, separators=(',', ':')))


def _render_slice(row):
    keys = {'render_start', 'render_end', 'rendered'}
    if not keys.intersection(row):
        return 0, len(row['text']), True
    start, end, visible = row.get('render_start'), row.get('render_end'), row.get('rendered')
    if (type(start) is not int or type(end) is not int or type(visible) is not bool
            or not 0 <= start <= end <= len(row['text'])):
        raise ValueError('invalid private source rendering slice')
    return start, end, visible


def _field_label(text):
    # A conservative form-field fence, never a rule for inferring decorations.
    return re.search(r'(?:公章|签字|签章|签名|日期|signature|date)[\s)）]*[:：]', text, re.I)


def load_decoration(artifacts, source, frozen, fact, occupied):
    from docreader.tender.directory_compile import _point, _window
    from docreader.parser.pdf_parser import _PDFIUM_LOCK
    import pypdfium2 as pdfium

    record = next((d for d in frozen['documents'] if d['document_id'] == fact['document_id']), None)
    if record is None or record['original_ref'] != fact['original_ref']:
        raise ArtifactIntegrityError('decoration must refer to its exact current frozen original')
    data = artifacts.read_bytes(fact['original_ref'])
    if not data.startswith(b'%PDF'):
        raise ArtifactIntegrityError('original character decorations currently require a PDF')
    fragments = []
    with _PDFIUM_LOCK, pdfium.PdfDocument(data) as pdf:
        for selected in fact['layout_fragments']:
            page, line = selected['page'], selected['line']
            text = source.line(fact['document_id'], page, line)
            start, end = selected['start'], selected['end']
            if not 0 <= start < end <= len(text) or page >= len(pdf):
                raise ArtifactIntegrityError('decoration layout fragment is outside its exact source line')
            value = text[start:end]
            if _sha(value) != selected['text_sha256'] or source.unit_at(fact['document_id'], page, line):
                raise ArtifactIntegrityError('decoration source changed or overlaps a native grid')
            if _field_label(value):
                raise ArtifactIntegrityError('a fixed signature or date field cannot be a decoration')
            point = (fact['document_id'], page, line)
            for first, last in occupied.get(point, []):
                if first < end and start < last:
                    raise ArtifactIntegrityError('reviewed decorations must not repeat or overlap')
            occupied.setdefault(point, []).append((start, end))
            handle = pdf[page]
            textpage = handle.get_textpage()
            try:
                original = textpage.get_text_range()
                chars = selected['original_chars']
                first, last = chars['start'], chars['end']
                if not 0 <= first < last <= len(original):
                    raise ArtifactIntegrityError('decoration original character fragment is outside its page')
                original = original[first:last]
                if (_sha(original) != chars['text_sha256']
                        or re.sub(r'\s+', '', original) != re.sub(r'\s+', '', value)):
                    raise ArtifactIntegrityError('decoration layout and original characters do not match')
                boxes = [textpage.get_charbox(i) for i in range(first, last) if original[i - first].strip()]
                if not boxes:
                    raise ArtifactIntegrityError('decoration needs actual original glyphs')
                fragments.append({**deepcopy(selected), 'text': value, 'line_text': text, 'bbox': [
                    min(b[0] for b in boxes), min(b[1] for b in boxes),
                    max(b[2] for b in boxes), max(b[3] for b in boxes)]})
            finally:
                textpage.close()
                handle.close()
    for approval in fact.get('context_approvals', []):
        point = _point(approval['owner_heading'], source, 'reviewed decoration context owner')
        window = _window(approval['context_window'], source, 'reviewed decoration context')
        raw = '\n'.join(source.line(window[0], window[1], line) for line in range(window[2], window[3] + 1))
        if (point[0] != fact['document_id'] or window[0] != fact['document_id']
                or not source.line(*point).strip() or _sha(raw) != approval['context_raw_rows_sha256']
                or not any(f['page'] == window[1] and window[2] <= f['line'] <= window[3] for f in fragments)):
            raise ArtifactIntegrityError('decoration context approval does not match its exact source and owner')
        artifacts.read_bytes(approval['review_evidence_ref'])
    return {**fact, '_fragments': fragments}


def validate_decoration_duties(artifacts, repository, facts, indexes, source):
    from docreader.tender.authoring_excerpts import load_source_excerpt

    def compact(value):
        return re.sub(r'\s+', '', value)
    for fact in (f for f in facts if f['kind'] == 'source_decoration'):
        for fragment in fact['_fragments']:
            left, bottom, right, top = fragment['bbox']
            for index in indexes:
                if index.document['document_id'] != fact['document_id']:
                    continue
                for unit in index.raw['units']:
                    box = unit.get('locator') or {}
                    if (unit.get('grid') and box.get('page_ordinal') == fragment['page']
                            and all(isinstance(box.get(k), (int, float)) for k in ('left', 'bottom', 'right', 'top'))
                            and left < box['right'] and box['left'] < right
                            and bottom < box['top'] and box['bottom'] < top):
                        raise ArtifactIntegrityError('decoration glyphs overlap a protected original grid')
            for body in (f for f in facts if f['kind'] == 'independent_body_response'):
                for excerpt in [*body['source_excerpts'], *body.get('context_source_excerpts', [])]:
                    loaded = load_source_excerpt(artifacts, excerpt, repository=repository)
                    if loaded['document']['document_id'] != fact['document_id']:
                        continue
                    index = repository.source_index(excerpt['source_ref']['source_index_ref'])
                    unit = next(u for u in index.raw['units'] if u['unit_id'] == excerpt['source_ref']['unit_id'])
                    page_text = source.pages(fact['document_id'])[fragment['page']]
                    unit_text = unit.get('text') or ''
                    offset = page_text.find(unit_text) if unit_text else -1
                    exact = offset >= 0 and page_text.find(unit_text, offset + 1) < 0
                    point_start = sum(len(line) for line in page_text.splitlines(keepends=True)[:fragment['line']])
                    selected_start, selected_end = point_start + fragment['start'], point_start + fragment['end']
                    for selected in loaded['ranges']:
                        box = selected.get('physical_locator') or {}
                        if box.get('page_ordinal') != fragment['page']:
                            continue
                        if exact and selected.get('text_basis') != 'grid_row':
                            if offset + selected['start'] < selected_end and selected_start < offset + selected['end']:
                                raise ArtifactIntegrityError('decoration conflicts with an explicitly reviewed body or context')
                            continue
                        if all(isinstance(box.get(k), (int, float)) for k in ('left', 'bottom', 'right', 'top')):
                            if not (left < box['right'] and box['left'] < right and bottom < box['top'] and box['bottom'] < top):
                                continue
                        if (compact(fragment['text']) in compact(selected['text'])
                                and any(w['document_id'] == fact['document_id'] and w['page'] == fragment['page']
                                    and w['start_line'] <= fragment['line'] <= w['end_line'] for w in body['source_windows'])):
                            fact.setdefault('_ambiguous_body_points', []).append((fragment['page'], fragment['line']))


def collect_source_renderings(chapters, compiled):
    """Remove private compiler hints before the public outline is validated."""
    records = []
    def walk(entries):
        for chapter in entries:
            for target in chapter['targets']:
                rows = target.pop('_source_rows', None)
                generated = target.pop('_source_generated', False)
                if rows is not None or generated:
                    records.append({'target_id': target['target_id'], 'chapter_id': chapter['chapter_id'],
                        'rows': rows or [], 'rows_sha256': _rows_sha(rows or []),
                        'generated': generated, 'purpose_sha256': _sha(target['purpose'])})
            walk(chapter['children'])
    walk(chapters)
    compiled['_source_renderings'] = records


def project_decorations(outline, compiled, facts):
    selected = [f for f in facts if f['kind'] == 'source_decoration']
    if not selected:
        return
    def walk(entries):
        for chapter in entries:
            yield chapter
            yield from walk(chapter['children'])
    chapters = list(walk(outline['chapters']))
    targets = {t['target_id']: (c, t) for c in chapters for t in c['targets']}
    positions = {t['target_id']: ordinal for c in chapters for ordinal, t in enumerate(c['targets'])}
    incoming = {identity for t in [*(t for c in chapters for t in c['targets']),
        *outline.get('unplaced_targets', [])] for identity in t.get('related_target_ids', [])}
    origins = {r['target_id']: r for r in compiled.get('_source_renderings', [])}
    by_point = {}
    applications = {}
    for fact in selected:
        applications[fact['fact_id']] = {'fact_id': fact['fact_id'], 'kind': fact['kind'],
            'state': 'unresolved' if fact.get('_ambiguous_body_points') else 'not_in_formal_body',
            'enterprise_confirmed': False, 'label': fact['label'],
            'review_evidence_ref': fact['review_evidence_ref'], 'original_ref': fact['original_ref'],
            'original_fragments': deepcopy(fact['_fragments']), 'target_ids': [],
            'affected_target_ids': [], 'removed_target_ids': [], 'final_chapter_ids': [], 'retired_case_ids': []}
        if fact.get('context_approvals'):
            applications[fact['fact_id']]['context_approvals'] = deepcopy(fact['context_approvals'])
        for fragment in fact['_fragments']:
            by_point.setdefault((fact['document_id'], fragment['page'], fragment['line']), []).append((fact, fragment))
    protected = {}
    for chapter in compiled['chapters']:
        for kind in ('heading', 'entry'):
            if chapter.get(kind):
                p = chapter[kind]
                protected.setdefault((p['document_id'], p['page'], p['line']), []).append((kind, chapter.get('heading'), None))
        for kind in ('condition', 'clause'):
            for window in chapter.get(kind + '_windows', []):
                for line in range(window['start_line'], window['end_line'] + 1):
                    protected.setdefault((window['document_id'], window['page'], line), []).append((kind, chapter.get('heading'), window))

    def blocked(fact, fragment):
        if (fragment['page'], fragment['line']) in fact.get('_ambiguous_body_points', []):
            return True
        declarations = protected.get((fact['document_id'], fragment['page'], fragment['line']), [])
        return any(kind in {'heading', 'entry'} or not any(a['context_kind'] == kind
            and a['owner_heading'] == owner and a['context_window'] == window
            for a in fact.get('context_approvals', [])) for kind, owner, window in declarations)

    def matches(row, fragment):
        return (row['text'] == fragment['line_text'] and 0 <= fragment['start'] < fragment['end'] <= len(row['text'])
            and _sha(row['text'][fragment['start']:fragment['end']]) == fragment['text_sha256'])
    missing = [identity for identity, (_, t) in targets.items() if t.get('manual_fill')
        and t.get('support_origins') == ['tender'] and not t.get('template_refs')
        and not t.get('source_excerpts') and not t.get('source_body_ref') and (identity not in origins
            or not origins[identity].get('rows') and not origins[identity].get('generated'))]
    if missing:
        for record in applications.values():
            record['state'] = 'unresolved'
    for identity, origin in origins.items():
        if identity not in targets or origin.get('generated'):
            continue
        chapter, target = targets[identity]
        rows = origin['rows']
        related = [(fact, fragment) for r in rows
            for fact, fragment in by_point.get((r['document_id'], r['page'], r['line']), [])]
        if not related:
            continue
        try:
            slices = [_render_slice(row) for row in rows]
            raw = '\n'.join(row['text'][start:end] for row, (start, end, visible) in zip(rows, slices)
                            if visible).strip()
        except (KeyError, TypeError, ValueError):
            raw = None
        valid = (not target.get('template_refs') and not target.get('source_excerpts') and not target.get('source_body_ref')
            and target.get('manual_fill') and target.get('support_origins') == ['tender']
            and origin.get('chapter_id') == chapter['chapter_id'] and origin.get('rows_sha256') == _rows_sha(rows)
            and _sha(target['purpose']) == origin['purpose_sha256'] and raw == target['purpose']
            and all(matches(r, fragment) and not blocked(fact, fragment) for r in rows
                for fact, fragment in by_point.get((r['document_id'], r['page'], r['line']), [])))
        if not valid:
            for f, _ in related:
                applications[f['fact_id']]['state'] = 'unresolved'
            continue
        rendered, applied = [], []
        for row, (start, end, visible) in zip(rows, slices):
            point = row['document_id'], row['page'], row['line']
            text = row['text'][start:end]
            for fact, fragment in sorted(by_point.get(point, []), key=lambda x: x[1]['start'], reverse=True):
                first, last = max(start, fragment['start']) - start, min(end, fragment['end']) - start
                if not visible or first >= last:
                    continue
                text = text[:first] + text[last:]
                applied.append(fact)
            if visible:
                rendered.append(text)
        text = '\n'.join(rendered).strip()
        if not text and (identity in incoming or any(target.get(key) for key in (
                'rule_refs', 'source_refs', 'source_excerpts', 'source_body_ref', 'template_refs', 'subject',
                'response_table', 'response_form_pending', 'related_target_ids'))):
            for fact, _ in related:
                applications[fact['fact_id']]['state'] = 'unresolved'
            continue
        for fact in applied:
            record = applications[fact['fact_id']]
            if record['state'] != 'unresolved':
                record['state'] = 'presentation_excluded'
            record['affected_target_ids'].append(identity)
            record['final_chapter_ids'].append(chapter['chapter_id'])
        if text:
            target['purpose'] = text
        else:
            retired = {'chapter_id': chapter['chapter_id'], 'target_ordinal': positions[identity],
                'target': deepcopy(target), 'source_rendering': deepcopy(origin),
                'decoration_fact_ids': sorted({fact['fact_id'] for fact in applied})}
            for fact in {fact['fact_id']: fact for fact in applied}.values():
                applications[fact['fact_id']].setdefault('retired_targets', []).append(deepcopy(retired))
            chapter['targets'].remove(target)
        for fact, _ in related:
            record = applications[fact['fact_id']]
            record['target_ids' if text else 'removed_target_ids'].append(identity)
    cases = {c['id']: c for c in compiled.get('_requirement_cases', [])}
    for index, case_id in compiled.get('_source_issue_cases', {}).items():
        issue = outline['issues'][index]
        case = cases.get(case_id, {})
        rows = case.get('_source_rows') or []
        if issue['code'] != 'outline_directory_requirement_unassigned' or not rows:
            continue
        if '\n'.join(r['text'] for r in rows).strip() != case.get('source_text'):
            continue
        points = {(w['document_id'], w['page'], line) for w in case.get('source_windows', [])
                  for line in range(w['start_line'], w['end_line'] + 1)}
        if any((r['document_id'], r['page'], r['line']) not in points for r in rows):
            continue
        related = []
        for row in rows:
            fragments = by_point.get((row['document_id'], row['page'], row['line']), [])
            if any(not matches(row, f) or blocked(fact, f) or applications[fact['fact_id']]['state'] == 'unresolved'
                    for fact, f in fragments):
                for fact, _ in fragments:
                    applications[fact['fact_id']]['state'] = 'unresolved'
                break
            covered = {i for _, f in fragments for i in range(f['start'], f['end'])}
            if any(i not in covered for i, char in enumerate(row['text']) if char.strip()):
                break
            related.extend(fragments)
        else:
            if related:
                outline['issues'][index] = {'code': 'outline_directory_source_decoration', 'rule_refs': [],
                    'detail': '已核页面装饰不作为企业响应事项；完整原文和核定证据保留在来源记录。'}
                for fact, _ in related:
                    record = applications[fact['fact_id']]
                    if record['state'] != 'unresolved':
                        record['state'] = 'presentation_excluded'
                    record['retired_case_ids'].append(case_id)
                    record.setdefault('origin_requirement_cases', []).append(deepcopy(case))
    records = compiled.setdefault('_reviewed_source_fact_applications', [])
    records[:] = [r for r in records if r['fact_id'] not in applications]
    for record in applications.values():
        if missing:
            record['state'] = 'unresolved'
            record['unmapped_target_ids'] = missing
        for field in ('target_ids', 'affected_target_ids', 'removed_target_ids', 'final_chapter_ids', 'retired_case_ids'):
            record[field] = list(dict.fromkeys(record[field]))
        if record['state'] == 'unresolved':
            outline['issues'].append({'code': 'outline_directory_reviewed_fact_pending', 'rule_refs': [],
                'detail': f'已核页面装饰“{record["label"]}”的正式呈现来源未完整绑定，保留原文；请核定精确来源位置后重试。'})
        records.append(record)
