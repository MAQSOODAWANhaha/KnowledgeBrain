"""One bounded review of the adopted submission tree, separate from content review."""

from __future__ import annotations

from copy import deepcopy

from pydantic import ValidationError

from docreader.llm.request import (
    ModelBudgetError,
    ModelContractError,
    ModelWorkUnitExhausted,
    build_request,
)
from docreader.tender.authoring import _OutlineContext, project_source_sections
from docreader.tender.schema import AUTHORING_GROUNDING_POLICY, AuthoringStructureResponse
from docreader.tender.prompts import execution_prompt


_COMPACT_VIEW_PROMPT = (
    "\nstructure_view=compact_v2时，support_sections按sections中的section_id分组，各组以原support_id为键；"
    "顶层supports以原support_id为键保留未分组支持。组内条目继承section_id。submission_sources以原source_id为键。"
    "条目为字符串时即完整text，否则读取其text/text_lines及其他字段。支持省略origin时为tender，grid和header_row_count省略时为null。"
    "章节省略is_volume时为false、presentation_role为body，targets/children省略时为空；"
    "目标省略manual_fill为false，related_target_ids/template_support_ids/source_support_ids/response_context_support_ids/"
    "response_rows/task_ids/rule_ids省略时为空数组。非默认字段仍原样提供。\n"
    "\n结构视图grid.rows保留全部单元格明文，行列从0开始；空字符串为原生空白格，"
    "null表示该位置没有独立单元格。merged_cells各项为[row,column,row_span,col_span]，"
    "保留所有非1×1合并；原始列宽由宿主保存。submission_sources中的grid与supports中的grid含义相同。\n"
    "条目用text_lines代替text时，字符串是原文行，整数是顶层text_pool从0开始的索引；"
    "按序以换行连接恢复完整原文，空行保留。共享原文不合并来源身份、主体、条件、选择或引用。\n"
)

_NATIVE_RESPONSE_PROMPT = (
    "\n原式native_response_layout的response_rows中，context_support_ids是名称、分值等上下文，"
    "source_support_ids只含该行条款正文；上下文与正文以换行分开，不得混为同一续句。"
    "source_join=paragraphs表示正文片段按序以换行连接，continuation表示按序直接续接且保留原片段空白；"
    "source_guidance同样按其source_join恢复完整参考原文，kind为参考规则或评审程序，不能作为企业已满足的应答。\n"
)

_RESPONSE_FORM_DECISION_PROMPT = (
    "\nresponse_form_decisions提供对应原式的展开决议及全部已给职责依据。核对keep_original/expand是否符合全部依据，"
    "不能只看被选中的依据索引；明确要求逐条列原文的原式仍为空表时应指出遗漏，"
    "只要求据实填写偏差或资料时不强迫转录全部招标条款。决议完成不表示企业事实已确认或已经响应。\n"
)

_RESPONSE_FORM_ROLE_PROMPT = (
    "\nresponse_form_contract=source_bound_response_forms_v3的response_form_decisions中，row_roles按实际原式行序绑定"
    "row_id及review_row_id；review_row_id对应此target的response_rows。source_role=criterion是投标人应答条款，"
    "reference是参考规则，procedure是评委或他方程序。核对全部mandate_scope要求的原文入表范围，"
    "程序可因全列职责进入原文列但不能变为企业义务、自评分或已执行；企业应答与证明格应留空。"
    "核对正文与source_guidance没有重复拥有同源条款，不得只因procedure/reference入表就强改criterion或删原文。\n"
)

_SOURCE_BODY_CONTEXT_PROMPT = (
    "\nsource_body_contract=source_bound_native_body_v1时，source_support_ids是精确正文片段，"
    "source_context_support_ids是独立保留的同原格上下文，不是企业应答。"
    "核对正文与上下文均完整且不串列，不将cell片段扩大为整行，也不以context代替独立正文职责。\n"
)

_RULE_FIELDS = (
    "text", "kind", "domains", "subject", "scope", "conditions", "exceptions",
    "numbers", "scoring", "consequence", "submission",
)


def _rule_dispositions(context, issues):
    from docreader.tender.authoring_pipeline import _json

    views = {rule["rule_id"]: rule for rule in context.payload["rules"]}
    by_ref = {
        _json(ref): views[identity]
        for identity, ref in context.rules.items()
        if identity in views
    }
    dispositions, gaps = [], []
    for issue in issues:
        if issue["code"] not in {"outline_rule_unassigned", "outline_rule_excluded"}:
            continue
        rules, refs = (
            [],
            list(dict.fromkeys(_json(ref) for ref in issue.get("rule_refs", []))),
        )
        unavailable = int(not refs)
        for reference in refs:
            view = by_ref.get(reference, {})
            semantic = view.get("effective_rule", view.get("item", {}))
            quotes = [
                {"quote": evidence["quote"]}
                for evidence in semantic.get("evidence", [])
                if evidence.get("quote", "").strip()
            ]
            if not semantic.get("text", "").strip() or not quotes:
                unavailable += 1
                continue
            rules.append(
                {
                    **{
                        field: deepcopy(semantic[field])
                        for field in _RULE_FIELDS
                        if field in semantic
                    },
                    "evidence": quotes,
                }
            )
        dispositions.append(
            {"code": issue["code"], "reason": issue["detail"], "rules": rules}
        )
        if unavailable:
            gaps.append(
                {
                    "code": "structure_rule_disposition_unavailable",
                    "detail": f"{unavailable}项未安排或排除规则的当前语义无法读取，保留原处置及核对缺口。",
                }
            )
    return dispositions, gaps


def _structure_payload(artifacts, outline, units, *, outline_phase=False, template_bindings=None):
    from docreader.tender.source_body import validate_outline_source_bodies
    validate_outline_source_bodies(artifacts, outline)
    from docreader.tender.authoring_pipeline import _constraint_context, _json
    from docreader.tender.authoring_templates import (
        load_template_part,
        template_part_key,
    )

    from docreader.tender.authoring_excerpts import load_source_excerpt, source_excerpt_key

    if outline_phase:
        units = []
    supports, part_ids, targets, gaps, task_ids = [], {}, {}, [], {}
    actual_targets = {}
    pending_rules, target_rules, rule_ids, rule_issues = [], {}, {}, []
    target_prefix = "v" if outline_phase else "t"
    for unit in units:
        for identity in unit.get("target_ids", []):
            task_ids.setdefault(identity, []).append(unit["task_id"])
    context = _OutlineContext(artifacts, outline["authoring_input"])
    submission = _constraint_context(
        context,
        outline.get("constraint_rule_refs", []),
        outline.get("submission_refs", []),
    )
    dispositions, disposition_gaps = _rule_dispositions(
        context, outline.get("issues", [])
    )
    gaps.extend(disposition_gaps)
    views = {rule["rule_id"]: rule for rule in context.payload["rules"]}
    rule_views = {_json(ref): views[identity] for identity, ref in context.rules.items() if identity in views}

    def register(entries):
        for chapter in entries:
            for target in chapter.get("targets", []):
                targets.setdefault(target["target_id"], f"{target_prefix}{len(targets) + 1}")
                actual_targets[target['target_id']] = target
            register(chapter.get("children", []))

    register(outline["chapters"])

    def source_supports(excerpts, chapter_id, target_id):
        identities = []
        fragments = [{"source_ref": excerpt["source_ref"], "selectors": [selected]}
                     for excerpt in excerpts for selected in excerpt["selectors"]]
        for excerpt in fragments:
            try:
                key = ("excerpt", source_excerpt_key(excerpt))
                if key not in part_ids:
                    loaded = load_source_excerpt(artifacts, excerpt, repository=context.repository)
                    source = loaded["unit"]
                    support_id = f"s{len(supports) + 1}"
                    supports.append({"support_id": support_id, "origin": "tender", "text": loaded["text"],
                        "heading_path": source.get("heading_path", source.get("locator", {}).get("heading_path", ""))})
                    part_ids[key] = support_id
                if part_ids[key] not in identities:
                    identities.append(part_ids[key])
            except (OSError, ValueError, KeyError):
                gaps.append({"code": "structure_source_unavailable",
                    "detail": "本位置的精确招标原文暂不可读，保留来源及结构待核。",
                    "chapter_id": chapter_id, "target_id": target_id})
        return identities

    def chapters(entries):
        result = []
        for chapter in entries:
            selected = []
            for target in chapter.get("targets", []):
                identity = targets.setdefault(
                    target["target_id"], f"{target_prefix}{len(targets) + 1}"
                )
                template_ids = []
                selected_bindings = {}
                for part in target.get("template_refs", []):
                    try:
                        key = template_part_key(part)
                        if key not in part_ids:
                            loaded = load_template_part(
                                artifacts, part, repository=context.repository
                            )
                            source = loaded["unit"]
                            if (
                                source["kind"] == "image_region"
                                and not loaded.get("text", "").strip()
                            ):
                                gaps.append(
                                    {
                                        "code": "structure_template_unavailable",
                                        "detail": "该原格式为图片且尚无可读OCR文本，结构核对保留来源缺口。",
                                        "chapter_id": chapter["chapter_id"],
                                        "target_id": target["target_id"],
                                    }
                                )
                                continue
                            support_id = f"s{len(supports) + 1}"
                            supports.append(
                                {
                                    "support_id": support_id,
                                    "origin": "tender",
                                    "text": loaded.get("text", source.get("text", "")),
                                    "grid": loaded.get("grid", source.get("grid")),
                                    "header_row_count": part.get("header_row_count"),
                                    "heading_path": source.get(
                                        "heading_path",
                                        source.get("locator", {}).get(
                                            "heading_path", ""
                                        ),
                                    ),
                                }
                            )
                            part_ids[key] = support_id
                        if part_ids[key] not in template_ids:
                            template_ids.append(part_ids[key])
                        selected_bindings[part_ids[key]] = key
                    except (OSError, ValueError, KeyError):
                        gaps.append(
                            {
                                "code": "structure_template_unavailable",
                                "detail": "本实例的实际格式暂不可读，结构核对保留来源缺口。",
                                "chapter_id": chapter["chapter_id"],
                                "target_id": target["target_id"],
                            }
                        )
                if template_bindings is not None:
                    template_bindings[identity] = {"target": target, "parts": selected_bindings}
                selected.append(
                    {
                        "target_id": identity,
                        "kind": target["kind"],
                        "purpose": target["purpose"],
                        **({"subject": target["subject"]} if target.get("subject") else {}),
                        "manual_fill": target.get("manual_fill", False),
                        "related_target_ids": [
                            targets[ref]
                            for ref in target.get("related_target_ids", [])
                            if ref in targets
                        ],
                        "template_support_ids": template_ids,
                        "task_ids": task_ids.get(target["target_id"], []),
                    }
                )
                if target.get("source_excerpts") and not target.get('source_body_contract'):
                    selected[-1]["source_support_ids"] = source_supports(target["source_excerpts"],
                        chapter["chapter_id"], target["target_id"])
                if target.get('source_body_contract'):
                    selected[-1]['source_body_contract'] = target['source_body_contract']
                    key = ('body', target['source_body_ref']['sha256'])
                    if key not in part_ids:
                        part_ids[key] = f's{len(supports) + 1}'
                        supports.append({'support_id': part_ids[key], 'origin': 'tender', 'text': target['purpose']})
                    selected[-1]['source_support_ids'] = [part_ids[key]]
                if target.get('source_body_contract') or target.get('project_context_excerpts'):
                    selected[-1]['source_context_support_ids'] = source_supports(
                        target.get('project_context_excerpts') or target.get('source_context_excerpts', []),
                        chapter['chapter_id'], target['target_id'])
                if target.get("response_table"):
                    if target["response_table"].get("context_source_excerpts"):
                        selected[-1]["response_context_support_ids"] = source_supports(
                            target["response_table"]["context_source_excerpts"], chapter["chapter_id"], target["target_id"])
                    layout = target['response_table'].get('native_layout')
                    if layout:
                        selected[-1]["response_rows"] = [{"row_id": f"q{ordinal}",
                            "context_support_ids": source_supports(row.get("context_source_excerpts", []),
                                chapter["chapter_id"], target["target_id"]),
                            "source_support_ids": source_supports(row["source_excerpts"],
                                chapter["chapter_id"], target["target_id"]),
                            "source_join": row.get("source_join", "paragraphs")}
                            for ordinal, row in enumerate(target["response_table"]["rows"], 1)]
                    else:
                        selected[-1]["response_rows"] = [{"row_id": f"q{ordinal}",
                            "source_support_ids": source_supports([*row.get("context_source_excerpts", []), *row["source_excerpts"]],
                                chapter["chapter_id"], target["target_id"])}
                            for ordinal, row in enumerate(target["response_table"]["rows"], 1)]
                    if layout:
                        keys = {template_part_key(part) for part in target.get('template_refs', [])
                                if part['source_ref'] == layout['source_ref']}
                        selected[-1]['native_response_layout'] = {
                            **{key: value for key, value in layout.items() if key != 'source_ref'},
                            'template_support_ids': [support for support, key in selected_bindings.items() if key in keys]}
                    if target['response_table'].get('source_guidance'):
                        selected[-1]['source_guidance'] = [
                            {'kind': entry['kind'], 'source_support_ids': source_supports(
                                entry['source_excerpts'], chapter['chapter_id'], target['target_id']),
                             **({'source_join': entry.get('source_join', 'paragraphs')} if layout else {})}
                            for entry in target['response_table']['source_guidance']]
                    pending_rules.extend((row_view, row.get("rule_refs", []), chapter["chapter_id"], target["target_id"])
                        for row_view, row in zip(selected[-1]["response_rows"], target["response_table"]["rows"]))
                pending_rules.append((selected[-1], target.get("rule_refs", []), chapter["chapter_id"], target["target_id"]))
            result.append(
                {
                    "chapter_id": chapter["chapter_id"],
                    "title": chapter["title"],
                    "is_volume": chapter.get("is_volume", False),
                    "targets": selected,
                    "presentation_role": chapter.get("presentation_role", "body"),
                    "children": chapters(chapter.get("children", [])),
                }
            )
        return result

    tree = chapters(outline["chapters"])
    support_texts = {entry["support_id"]: entry.get("text", "") for entry in supports}
    unavailable_targets = set()
    for selected, references, chapter_id, target_id in pending_rules:
        selected["rule_ids"], unavailable = [], False
        for reference in references:
            key = _json(reference)
            view = rule_views.get(key, {})
            semantic = view.get("effective_rule", view.get("item", {}))
            if not semantic.get("text", "").strip():
                unavailable = True
                continue
            if key not in rule_ids:
                evidence = []
                for quote in semantic.get("evidence", []):
                    if not quote.get("quote", "").strip():
                        continue
                    try:
                        fragments = [{"source_ref": excerpt["source_ref"], "selectors": [selector]}
                            for excerpt in getattr(context, "excerpts", {}).get(quote.get("range_id"), [])
                            for selector in excerpt["selectors"]]
                        support_ids = [part_ids.get(("excerpt", source_excerpt_key(excerpt))) for excerpt in fragments]
                    except (ValueError, KeyError):
                        support_ids = []
                    if support_ids and all(support_ids) and "\n".join(support_texts[identity] for identity in support_ids) == quote["quote"]:
                        evidence.append({"support_ids": support_ids})
                    else:
                        evidence.append({"quote": quote["quote"]})
                rule_ids[key] = f"r{len(rule_ids) + 1}"
                target_rules[rule_ids[key]] = {
                    **{field: deepcopy(semantic[field]) for field in _RULE_FIELDS if field in semantic},
                    "evidence": evidence,
                }
            identity = rule_ids[key]
            if identity not in selected["rule_ids"]:
                selected["rule_ids"].append(identity)
            unavailable = unavailable or not target_rules[identity]["evidence"] or (
                len(target_rules[identity]["evidence"]) != len(semantic.get("evidence", [])))
        if unavailable and target_id not in unavailable_targets:
            unavailable_targets.add(target_id)
            issue = {"code": "structure_target_rule_unavailable",
                "detail": "本位置部分规则的当前语义或引文无法读取，保留规则引用及结构待核。"}
            gaps.append({**issue, "chapter_id": chapter_id, "target_id": target_id})
            rule_issues.append({**issue, "target_id": targets[target_id]})
    summaries = [
        {
            "task_id": unit["task_id"],
            "chapter_id": unit["chapter_id"],
            "target_ids": [
                targets[identity]
                for identity in unit.get("target_ids", [])
                if identity in targets
            ],
            "status": unit["status"],
            "blocks_count": len(unit.get("blocks", [])),
            "cell_fills_count": len(unit.get("cell_fills", [])),
            "attachment_titles": [
                attachment["title"] for attachment in unit.get("attachments", [])
            ],
            "error_type": unit.get("error_type"),
        }
        for unit in units
    ]
    sections, supports = project_source_sections(supports)
    form_decisions = []
    for issue in outline.get('issues', []):
        if issue['code'] != 'outline_directory_response_form_decision':
            continue
        identity = targets.get(issue.get('target_id'))
        form_decisions.append({'target_id': identity, 'detail': issue['detail']})
        if identity is None:
            gaps.append({'code': 'structure_response_form_decision_unbound',
                         'detail': '原式展开决议没有对应当前响应位置，须核定原式及完整依据。'})
    return {
        "grounding_policy": AUTHORING_GROUNDING_POLICY,
        "operation": "structure",
        **({"phase": "outline"} if outline_phase else {}),
        "chapters": tree,
        "sections": sections,
        "supports": supports,
        "target_rules": target_rules,
        "units": summaries,
        "rule_dispositions": dispositions,
        "input_issues": [*context.availability_issues, *disposition_gaps, *rule_issues],
        **({'response_form_decisions': form_decisions} if form_decisions else {}),
        **submission,
    }, gaps


def _move_chapters(outline, moves):
    """Validate the complete edit batch before replacing any tree structure."""
    nodes, parents, siblings = {}, {}, {None: []}
    stack = [(outline['chapters'], None)]
    while stack:
        entries, parent = stack.pop()
        for node in entries:
            identity = node['chapter_id']
            if identity in nodes:
                raise ValueError('原大纲章节身份重复。')
            nodes[identity], parents[identity] = node, parent
            siblings[parent].append(identity)
            siblings[identity] = []
            stack.append((node['children'], identity))
    selected, anchors = set(), set()
    for move in moves:
        identity, parent, after = (move[key] for key in ('chapter_id', 'parent_chapter_id', 'after_chapter_id'))
        if identity not in nodes or parent is not None and parent not in nodes or after is not None and after not in nodes:
            raise ValueError('章节移动引用不属于本次大纲。')
        if identity in selected or identity == parent or identity == after:
            raise ValueError('章节重复移动或引用自身。')
        if (parent, after) in anchors:
            raise ValueError('多个移动争用同一父章下的位置。')
        selected.add(identity)
        anchors.add((parent, after))
        parents[identity] = parent
    checked = set()
    for identity in nodes:
        path, cursor = set(), identity
        while cursor is not None and cursor not in checked:
            if cursor in path:
                raise ValueError('移动后的父子关系存在循环。')
            path.add(cursor)
            cursor = parents[cursor]
        checked.update(path)
    for move in moves:
        after = move['after_chapter_id']
        if after is not None and parents[after] != move['parent_chapter_id']:
            raise ValueError('相对位置引用不属于移动后的同一父章。')
    for parent in siblings:
        siblings[parent] = [identity for identity in siblings[parent] if identity not in selected]
    pending = list(moves)
    while pending:
        waiting = []
        for move in pending:
            identity, parent, after = (move[key] for key in ('chapter_id', 'parent_chapter_id', 'after_chapter_id'))
            group = siblings[parent]
            if after is not None and after not in group:
                waiting.append(move)
            else:
                group.insert(0 if after is None else group.index(after) + 1, identity)
        if len(waiting) == len(pending):
            raise ValueError('相对位置依赖存在循环。')
        pending = waiting
    copies = {identity: deepcopy({key: value for key, value in node.items() if key != 'children'})
              for identity, node in nodes.items()}
    for identity, node in copies.items():
        node['children'] = [copies[child] for child in siblings[identity]]
    outline['chapters'] = [copies[identity] for identity in siblings[None]]


def _rename_chapters(outline, renames):
    """Validate every chapter reference before applying title-only corrections."""
    nodes = {}
    stack = list(outline['chapters'])
    while stack:
        node = stack.pop()
        identity = node['chapter_id']
        if identity in nodes:
            raise ValueError('原大纲章节身份重复。')
        nodes[identity] = node
        stack.extend(node['children'])
    selected = set()
    for rename in renames:
        identity = rename['chapter_id']
        if identity not in nodes or identity in selected:
            raise ValueError('章节改名引用不属于本次大纲或重复。')
        selected.add(identity)
    for rename in renames:
        nodes[rename['chapter_id']]['title'] = rename['title']


async def review_structure(artifacts, outline, units, budget, execute, *, outline_phase=False,
                           apply_source_decoration_exclusions=False, apply_hierarchy_edits=False,
                           apply_title_edits=False):
    """Return findings/gaps; the durable executor owns the existing retry allowance.

    Inspect final adopted instances once, including manual or unfilled forms.
    Do not send enterprise prose or turn structure feedback into another rewrite.
    Source decoration exclusions execute only when automatic organization opts in.
    """
    if not outline.get("chapters"):
        return [], []
    template_bindings = {}
    if outline_phase:
        units = []
        payload, gaps = _structure_payload(artifacts, outline, units, outline_phase=True,
                                            template_bindings=template_bindings)
    else:
        payload, gaps = _structure_payload(artifacts, outline, units, template_bindings=template_bindings)
    if apply_source_decoration_exclusions:
        payload["source_decoration_edits_allowed"] = True
    if apply_hierarchy_edits:
        payload["hierarchy_edits_allowed"] = True
    if apply_title_edits:
        payload["title_edits_allowed"] = True
    task_ids = {unit["task_id"] for unit in units}

    def validate(raw):
        try:
            value = AuthoringStructureResponse.model_validate(raw).model_dump()
        except ValidationError as error:
            raise ModelContractError(
                [
                    {"path": list(e["loc"]), "code": e["type"], "detail": e["msg"]}
                    for e in error.errors(include_input=False)
                ]
            ) from None
        for finding in value["findings"]:
            if set(finding["task_ids"]) - task_ids or finding["content_ids"]:
                raise ModelContractError(
                    [
                        {
                            "path": ["findings"],
                            "code": "structure_unknown_reference",
                            "detail": "Select only supplied tasks; this review has no block or fill content references.",
                        }
                    ]
                )
            finding["task_ids"] = list(dict.fromkeys(finding["task_ids"]))
            finding["reject_content"] = False
            if not outline_phase:
                finding["can_revise"] = False
        seen, exclusions = set(), []
        for ordinal, action in enumerate(value["source_decoration_exclusions"]):
            target_id = action["target_id"]
            path = ["source_decoration_exclusions", ordinal]
            if target_id not in template_bindings:
                raise ModelContractError([{"path": [*path, "target_id"],
                    "code": "structure_unknown_reference", "detail": "Select a supplied structure target."}])
            for number, support_id in enumerate(action["support_ids"]):
                if support_id not in template_bindings[target_id]["parts"]:
                    raise ModelContractError([{"path": [*path, "support_ids", number],
                        "code": "structure_unknown_reference",
                        "detail": "Select only this target's template_support_ids, not source/context supports."}])
            support_ids = []
            for support_id in action["support_ids"]:
                if (target_id, support_id) not in seen:
                    seen.add((target_id, support_id))
                    support_ids.append(support_id)
            if support_ids:
                exclusions.append({**action, "support_ids": support_ids})
        value["source_decoration_exclusions"] = exclusions
        return value

    def accepted(value):
        from docreader.tender.authoring_templates import template_part_key

        excluded = {}
        for action in value["source_decoration_exclusions"]:
            binding = template_bindings[action["target_id"]]
            if not apply_source_decoration_exclusions:
                gaps.append({"code": "structure_source_decoration_exclusion_not_applied",
                    "detail": ("本次固定大纲核对仅保留独立源装饰排除建议（" + action["target_id"]
                               + "：" + "、".join(action["support_ids"]) + "）：" + action["reason"]),
                    "target_id": binding["target"]["target_id"]})
                continue
            excluded.setdefault(action["target_id"], set()).update(
                binding["parts"][identity] for identity in action["support_ids"])
        for identity, keys in excluded.items():
            target = template_bindings[identity]["target"]
            target["template_refs"] = [part for part in target.get("template_refs", [])
                                       if template_part_key(part) not in keys]
        moves = value['chapter_moves']
        if moves:
            if not apply_hierarchy_edits:
                gaps.append({'code': 'structure_hierarchy_edits_not_applied',
                    'detail': '本次结构核对仅保留章节移动建议，不修改固定大纲或已保存Word：' +
                              '；'.join(move['chapter_id'] + '：' + move['reason'] for move in moves)})
            else:
                try:
                    _move_chapters(outline, moves)
                except ValueError as error:
                    gaps.append({'code': 'structure_hierarchy_edits_invalid',
                        'detail': f'章节移动未应用，保留原结构及复核意见待核：{error}'})
                else:
                    gaps.extend({'code': 'structure_hierarchy_edits_applied', 'chapter_id': move['chapter_id'],
                        'detail': '已执行结构复核明确选择的章节移动，原语义意见仍保留待核：' + move['reason']}
                        for move in moves)
        renames = value['chapter_renames']
        if renames:
            if not apply_title_edits:
                gaps.append({'code': 'structure_title_edits_not_applied',
                    'detail': '本次结构核对仅保留标题建议，不修改固定大纲或已保存Word：' +
                              '；'.join(rename['chapter_id'] + '：' + rename['reason'] for rename in renames)})
            else:
                try:
                    _rename_chapters(outline, renames)
                except ValueError as error:
                    gaps.append({'code': 'structure_title_edits_invalid',
                        'detail': f'章节改名未应用，保留原标题及复核意见待核：{error}'})
                else:
                    gaps.extend({'code': 'structure_title_edits_applied', 'chapter_id': rename['chapter_id'],
                        'detail': '已执行结构复核明确选择的业务标题：' + rename['reason']}
                        for rename in renames)
        return value["findings"], gaps

    prompt = execution_prompt(budget.prompts, "authoring_structure")
    chapters = list(payload.get('chapters', []))
    if payload.get('response_form_decisions'):
        prompt += _RESPONSE_FORM_DECISION_PROMPT
    if any(record.get('response_form_contract') == 'source_bound_response_forms_v3'
           for record in payload.get('response_form_decisions', [])):
        prompt += _RESPONSE_FORM_ROLE_PROMPT
    native_response, source_body = False, False
    while chapters:
        chapter = chapters.pop()
        if any(target.get('native_response_layout') for target in chapter.get('targets', [])):
            native_response = True
        if any(target.get('source_body_contract') for target in chapter.get('targets', [])):
            source_body = True
        chapters.extend(chapter.get('children', []))
    if native_response:
        prompt += _NATIVE_RESPONSE_PROMPT
    if source_body:
        prompt += _SOURCE_BODY_CONTEXT_PROMPT
    request = build_request(budget.model, prompt, payload, task="authoring_structure",
                            reasoning_effort=budget.reasoning_effort)
    try:
        from docreader.tender.authoring_pipeline import _plan, _replay

        await _plan(execute, "authoring_structure", 1, "structure")
        hit, value = await _replay(execute, "authoring_structure", request, validate)
        if hit:
            return accepted(value)
        try:
            budget.require_request(request, "authoring_structure")
        except ModelBudgetError:
            from docreader.tender.authoring_view import compact_structure_payload

            request = build_request(
                budget.model,
                prompt + _COMPACT_VIEW_PROMPT,
                compact_structure_payload(payload),
                task="authoring_structure",
                reasoning_effort=budget.reasoning_effort,
            )
            hit, value = await _replay(execute, "authoring_structure", request, validate)
            if hit:
                return accepted(value)
            budget.admit_request(request, "authoring_structure")
        value = await execute("authoring_structure", request, validate)
    except (ModelBudgetError, ModelWorkUnitExhausted) as error:
        gaps.append(
            {
                "code": "structure_review_unavailable",
                "detail": f"实际提交结构尚未完成核对（{type(error).__name__}），保留草稿及原有内容。",
            }
        )
        return [], gaps
    return accepted(value)
