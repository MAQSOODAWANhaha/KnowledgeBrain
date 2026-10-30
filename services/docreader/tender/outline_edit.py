"""Apply human tree edits while retaining server-owned evidence and target identity."""

from copy import deepcopy
from uuid import UUID

from pydantic import Field, StrictBool, StrictStr

from docreader.tender.schema import AuthoringOutline, ChapterPresentationRole, Contract, Identity, SourceUnitRef, Text


class OutlineSourceAddition(Contract):
    target_id: Identity
    source_refs: list[SourceUnitRef] = Field(min_length=1, max_length=1000)


class OutlineEditChapter(Contract):
    chapter_id: Identity = Field(max_length=256)
    title: Text = Field(max_length=1000)
    children: list['OutlineEditChapter'] = Field(default_factory=list)
    is_volume: StrictBool | None = None
    presentation_role: ChapterPresentationRole | None = None
    writing_goal: StrictStr = Field(default='', max_length=20000)
    target_ids: list[Identity] | None = Field(default=None, max_length=1000)
    source_additions: list[OutlineSourceAddition] = Field(default_factory=list, max_length=1000)


def _flatten(chapters):
    for chapter in chapters:
        yield chapter
        yield from _flatten(chapter['children'])


def _check_size(chapters):
    if not isinstance(chapters, list):
        raise ValueError('章节必须是列表。')
    stack, count = [(chapters, 1)], 0
    while stack:
        nodes, depth = stack.pop()
        if depth > 16:
            raise ValueError('大纲层级最多16层，请先简化章节结构。')
        if not isinstance(nodes, list):
            raise ValueError('子章节必须是列表。')
        for node in nodes:
            count += 1
            if count > 1000:
                raise ValueError('大纲最多1000章，请先合并部分章节。')
            if not isinstance(node, dict):
                raise ValueError('章节必须是对象。')
            if node.get('children'):
                stack.append((node['children'], depth + 1))


def _manual_id(identity):
    if not identity.startswith('manual-'):
        return False
    try:
        return str(UUID(identity[7:])) == identity[7:]
    except ValueError:
        return False


def apply_outline_edit(base, chapters, *, target_catalog=()):
    """Only the editable projection crosses the API; references come from base."""
    _check_size(chapters)
    base = AuthoringOutline.model_validate(deepcopy(base)).model_dump(exclude_unset=True)
    originals = list(_flatten(base['chapters']))
    known = {chapter['chapter_id']: chapter for chapter in originals}
    original_targets = {target['target_id']: target for chapter in originals for target in chapter['targets']}
    available = {target['target_id']: deepcopy(target) for target in target_catalog}
    available.update(original_targets)
    sources = [*base.get('submission_refs', []),
               *(ref for target in available.values() for ref in target['source_refs'])]
    if (len(known) != len(originals)
            or len(original_targets) != sum(len(chapter['targets']) for chapter in originals)):
        raise ValueError('原大纲存在重复标识，无法确定章节或响应位置，请重新生成大纲。')
    edits = [OutlineEditChapter.model_validate(value) for value in chapters]
    seen = set()

    def bind(node):
        identity = node.chapter_id
        if identity in seen:
            raise ValueError('章节标识重复或循环，请调整章节结构。')
        seen.add(identity)
        original = known.get(identity)
        if original is None and not _manual_id(identity):
            raise ValueError('新增章节须使用独立的manual-UUID标识。')
        chapter = deepcopy(original) if original is not None else {
            'chapter_id': identity, 'targets': [], 'is_volume': False, 'presentation_role': 'body'}
        chapter['title'] = node.title
        chapter['children'] = [bind(child) for child in node.children]
        if node.target_ids is not None:
            if any(target_id not in available for target_id in node.target_ids):
                raise ValueError('响应位置不属于此大纲快照，请载入对应历史修订后重新安排。')
            chapter['targets'] = [deepcopy(available[target_id]) for target_id in node.target_ids]
        for field in ('is_volume', 'presentation_role'):
            if field in node.model_fields_set:
                chapter[field] = getattr(node, field)
        if 'writing_goal' in node.model_fields_set:
            target_id = 'manual-response-' + identity
            if (not _manual_id(identity) or any(target['target_id'] != target_id
                    or target['rule_refs'] or target['source_refs'] or target.get('template_refs', [])
                    for target in chapter['targets'])):
                raise ValueError('既有响应位置须保留原说明和引用；可新增章节填写编写说明。')
            chapter['targets'] = ([{'target_id': target_id, 'kind': 'narrative',
                'purpose': node.writing_goal, 'rule_refs': [], 'source_refs': [], 'template_refs': [],
                'manual_fill': False, 'support_origins': ['knowledge'], 'related_target_ids': []}]
                if node.writing_goal.strip() else [])
        targets = {target['target_id']: target for target in chapter['targets']}
        for addition in node.source_additions:
            if addition.target_id not in available or addition.target_id not in targets:
                raise ValueError('追加来源只能选择本章已有或选入的响应位置。')
            target = targets[addition.target_id]
            for source in addition.source_refs:
                reference = source.model_dump()
                if reference not in sources:
                    raise ValueError('追加来源不属于此大纲快照，请选择已有的完整来源引用。')
                if reference not in target['source_refs']:
                    target['source_refs'].append(reference)
        return chapter

    base['chapters'] = [bind(node) for node in edits]
    remaining = {target['target_id']: target for chapter in _flatten(base['chapters'])
                 for target in chapter['targets']}
    if len(remaining) != sum(len(chapter['targets']) for chapter in _flatten(base['chapters'])):
        raise ValueError('响应位置标识重复，请重新选择章节标识。')
    base['issues'] = [issue for issue in base['issues']
                      if issue['code'] != 'outline_edit_reference_unbound'
                      and (issue['code'] != 'outline_edit_target_removed' or issue.get('target_id') not in remaining)]
    for identity in original_targets.keys() - remaining.keys():
        target = original_targets[identity]
        base['issues'].append({'code': 'outline_edit_target_removed', 'target_id': identity,
            'detail': '大纲编排已移出响应位置“' + target['purpose'] + '”；原要求和来源仍保留，须重新安排或核对。',
            'rule_refs': deepcopy(target['rule_refs'])})
    for target in remaining.values():
        missing = set(target.get('related_target_ids', [])) - remaining.keys()
        if missing:
            base['issues'].append({'code': 'outline_edit_reference_unbound', 'target_id': target['target_id'],
                'detail': '此响应位置引用的部分位置已移出大纲，需重新安排或核对。',
                'rule_refs': deepcopy(target['rule_refs'])})
    return AuthoringOutline.model_validate(base).model_dump(exclude_unset=True)
