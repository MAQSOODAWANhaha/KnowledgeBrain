"""Name source-owned chapters without using a display name as their identity."""

from copy import deepcopy
import hashlib
import json
import re

from pydantic import ValidationError

from docreader.tender.directory_compile import DirectoryChapterTitle, DirectoryPlanRejected, _Source, _point


_SOURCE_NUMBER = re.compile(r'^(?:(?:附件|附录|附表)\s*[A-Za-z0-9一二三四五六七八九十]|第[0-9一二三四五六七八九十]+[卷章节部分]|[0-9一二三四五六七八九十]+[.．、])')
_SOURCE_LOCATOR = re.compile(r'(?:附件|附录|附表)\s*[A-Za-z0-9一二三四五六七八九十]+|[（(]\s*[0-9]+[A-Za-z]\s*[)）]\s*$')


def business_title(value):
    if (not isinstance(value, str) or not value.strip() or len(value.strip()) > 40
            or any(character in value for character in '\r\n。；;：:…★☑□') or '...' in value
            or _SOURCE_NUMBER.match(value.strip()) or _SOURCE_LOCATOR.search(value)
            or re.fullmatch(r'[0-9]+[A-Za-z0-9.]*', value.strip())):
        raise DirectoryPlanRejected('business title must be a complete short name without source or host numbering')
    return value.strip()


def _identity(kind, value):
    encoded = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':'))
    return kind + '-title-' + hashlib.sha256(encoded.encode()).hexdigest()


def chapter_title_id(chapter):
    return _identity('chapter', chapter['heading'])


def group_identity(chapter):
    heading = chapter.get('group_heading')
    return _identity('group', heading if heading is not None else {
        'document_id': chapter['heading']['document_id'], 'source_name': chapter.get('group')})


def _entries(chapters, source):
    known = {chapter['id'] for chapter in chapters}
    for chapter in chapters:
        if chapter.get('unplaced'):
            continue
        heading = chapter['heading']
        position = _point(heading, source, 'chapter title')
        yield {'title_id': chapter_title_id(chapter), 'kind': 'chapter', 'heading': deepcopy(heading),
               'source_title': source.line(*position), 'adopted_source_name': chapter['title'],
               'response_kind': chapter.get('response_kind', 'form'),
               'subjects': [subject['role'] for subject in chapter.get('subjects', [])]}
        group = (chapter.get('group') or '').strip()
        if not group or group == chapter['title'] or chapter.get('parent_id') in known:
            continue
        heading = chapter.get('group_heading')
        title = source.line(*_point(heading, source, 'group title')) if heading is not None else group
        yield {'title_id': group_identity(chapter), 'kind': 'group', 'heading': deepcopy(heading),
               'source_title': title, 'adopted_source_name': group}


def _selected_catalog(compiled, source):
    return {entry['title_id']: entry for entry in _entries(compiled['chapters'], source)}


def title_catalog(compiled, documents):
    """Offer current and finite alternate source identities before material choices."""
    source = _Source(documents)
    entries = _selected_catalog(compiled, source)
    for issue in compiled['issues']:
        case = issue.get('_directory_case') or {}
        for option in ('current', 'alternative'):
            candidate = case.get(option)
            if candidate is not None:
                for entry in _entries([candidate], source):
                    entries.setdefault(entry['title_id'], entry)
    return list(entries.values())


def validate_chapter_titles(compiled, documents, values, offered_catalog=None):
    required = _selected_catalog(compiled, _Source(documents))
    offered = {entry['title_id'] for entry in (
        title_catalog(compiled, documents) if offered_catalog is None else offered_catalog)}
    if not set(required) <= offered or not isinstance(values, list):
        raise DirectoryPlanRejected('chapter titles must bind the offered final source candidates')
    selected = {}
    for raw in values:
        try:
            value = DirectoryChapterTitle.model_validate(raw).model_dump()
        except ValidationError as error:
            raise DirectoryPlanRejected('invalid chapter title: ' + str(error)) from None
        identity = value['title_id']
        if identity not in offered or identity in selected:
            raise DirectoryPlanRejected('chapter title identity is unknown or repeated')
        value['title'] = business_title(value['title'])
        selected[identity] = value
    if not set(required) <= set(selected):
        raise DirectoryPlanRejected('chapter titles must cover every final chapter and source group')
    return list(selected.values())


def title_mapping_issue(entry, title):
    heading = entry.get('heading')
    location = (f'{heading["document_id"]}第{heading["page"] + 1}页第{heading["line"] + 1}行'
                if heading else '原分组来源锚未记录')
    return {'code': 'outline_directory_title_mapping', '_title_id': entry['title_id'],
            'detail': f'章节名称“{title}”对应来源原名“{entry["source_title"]}”；{location}；来源身份：{entry["title_id"]}。'}


def apply_chapter_titles(compiled, documents, values, offered_catalog=None):
    selected = {value['title_id']: value['title'] for value in validate_chapter_titles(
        compiled, documents, values, offered_catalog)}
    required = _selected_catalog(compiled, _Source(documents))
    for chapter in compiled['chapters']:
        if chapter.get('unplaced'):
            continue
        chapter['display_title'] = selected[chapter_title_id(chapter)]
        if chapter.get('group') and group_identity(chapter) in required:
            chapter['group_display_title'] = selected[group_identity(chapter)]
    compiled['issues'] = [issue for issue in compiled['issues']
                          if issue.get('_title_id') not in required]
    compiled['issues'].extend(title_mapping_issue(entry, selected[identity])
                              for identity, entry in required.items())
    return compiled


def accept_parameter_titles(plan):
    """The new formal receipt requires names; legacy raw parsing remains compatible."""
    for chapter in plan.get('chapters', []):
        for item in chapter.get('items', []):
            business_title(item.get('title'))
    return plan
