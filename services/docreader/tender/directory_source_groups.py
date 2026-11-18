"""Restore missing submission groups from cited source, preserving adopted subtrees."""

from copy import deepcopy

from pydantic import Field

from docreader.llm.request import ModelContractError
from docreader.tender.directory_compile import (
    DirectoryPlanRejected, DirectoryPoint, DirectoryWindow, NativeMaterialPlan,
    _Source, _business_name, _point, _window, _window_text,
)
from docreader.tender.directory_titles import business_title, group_identity, title_mapping_issue
from docreader.tender.schema import Contract, Identity, Text


SOURCE_GROUP_CONTRACT = 'source_bound_submission_groups_v1'
SOURCE_GROUP_OPERATION = 'select_response_forms_and_groups_v2'


def _reject(detail, path):
    raise ModelContractError([{'path': ['source_groups', *path],
                              'code': 'directory_source_group_invalid', 'detail': detail}])


class DirectorySourceGroup(Contract):
    heading: DirectoryPoint
    basis_windows: list[DirectoryWindow] = Field(min_length=1)
    chapter_ids: list[Identity] = Field(min_length=1)
    title: Text = Field(max_length=40)


class MaterialSourceGroupPlan(NativeMaterialPlan):
    source_groups: list[DirectorySourceGroup] = Field(default_factory=list)


def source_group_candidates(compiled):
    return [{'chapter_id': chapter['id'], 'heading': deepcopy(chapter['heading']),
             'parent_id': chapter.get('parent_id'), 'grouped': bool(chapter.get('group')),
             'presentation': chapter.get('presentation')}
            for chapter in compiled['chapters'] if not chapter.get('unplaced')]


def validate_source_groups(compiled, documents, selections):
    source = _Source(documents)
    known = {chapter['id']: chapter for chapter in compiled['chapters']}
    existing = {(heading['document_id'], heading['page'], heading['line'])
                for chapter in known.values() if chapter.get('group')
                and (heading := chapter.get('group_heading')) is not None}
    groups, members, result = set(), set(), []
    for number, raw in enumerate(selections):
        selected = DirectorySourceGroup.model_validate(raw).model_dump()
        try:
            selected['title'] = business_title(selected['title'])
        except DirectoryPlanRejected as error:
            _reject(error.detail, (number, 'title'))
        try:
            heading = _point(selected['heading'], source, 'source group heading')
        except DirectoryPlanRejected as error:
            _reject(error.detail, (number, 'heading'))
        if not source.line(*heading).strip() or heading in groups | existing:
            _reject('source groups need a unique, nonempty, previously ungrouped source anchor', (number, 'heading'))
        groups.add(heading)
        windows, covered = [], set()
        for index, raw_window in enumerate(selected['basis_windows']):
            try:
                value = _window(raw_window, source, 'source group basis')
            except DirectoryPlanRejected as error:
                _reject(error.detail, (number, 'basis_windows', index))
            if value[0] != heading[0] or not _window_text(source, value).strip():
                _reject('a source group basis must cite nonempty source in its heading document', (number, 'basis_windows', index))
            windows.append(value)
            document_id, page, start, end = value
            lines = {(document_id, page, line) for line in range(start, end + 1)}
            if covered & lines:
                _reject('source group basis windows must not repeat source lines', (number, 'basis_windows', index))
            covered.update(lines)
        if not any(window[1] == heading[1] and window[2] <= heading[2] <= window[3] for window in windows):
            _reject('a source group basis must include its exact heading anchor', (number, 'heading'))
        for index, identity in enumerate(selected['chapter_ids']):
            chapter = known.get(identity)
            if (chapter is None or identity in members or chapter.get('unplaced')
                    or chapter.get('parent_id') in known or chapter.get('group')
                    or chapter.get('presentation') in ('cover', 'content')
                    or chapter['heading']['document_id'] != heading[0]):
                _reject('source groups may each select only supplied, ungrouped, same-document roots once; covers and content stay in place',
                        (number, 'chapter_ids', index))
            members.add(identity)
        result.append(selected)
    return sorted(result, key=lambda value: source.position(*_point(value['heading'], source, 'source group order')))


def apply_source_groups(compiled, documents, selections):
    """Validate the complete batch before changing any source-owned chapter."""
    selected = validate_source_groups(compiled, documents, selections)
    source = _Source(documents)
    known = {chapter['id']: chapter for chapter in compiled['chapters']}
    for group in selected:
        heading = _point(group['heading'], source, 'source group heading')
        source_title = source.line(*heading)
        for identity in group['chapter_ids']:
            known[identity].update(group=_business_name(source_title) or source_title.strip(), group_heading=deepcopy(group['heading']),
                group_display_title=group['title'], group_basis_windows=deepcopy(group['basis_windows']))
        representative = known[group['chapter_ids'][0]]
        entry = {'title_id': group_identity(representative), 'heading': group['heading'], 'source_title': source_title}
        compiled['issues'].append(title_mapping_issue(entry, group['title']))
        basis = '\n'.join(_window_text(source, _window(window, source, 'source group basis'))
                          for window in group['basis_windows'])
        locations = '；'.join(f'{window["document_id"]}第{window["page"] + 1}页第{window["start_line"] + 1}–{window["end_line"] + 1}行'
                             for window in group['basis_windows'])
        compiled['issues'].append({'code': 'outline_directory_source_decision',
            'detail': f'分册“{group["title"]}”采用原件明确分组，既有章内正文与主体保留；依据：{locations}。\n{basis}'})
    if selected:
        compiled.setdefault('_source_groups', []).extend(deepcopy(selected))
    return compiled
