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
from docreader.tender.schema import AUTHORING_GROUNDING_POLICY, AuthoringReviewResponse


_COMPACT_VIEW_PROMPT = (
    "\n结构视图grid.rows保留全部单元格明文，行列从0开始；空字符串为原生空白格，"
    "null表示该位置没有独立单元格。merged_cells各项为[row,column,row_span,col_span]，"
    "保留所有非1×1合并；原始列宽由宿主保存。submission_sources中的grid与supports中的grid含义相同。\n"
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
                        for field in (
                            "text",
                            "kind",
                            "domains",
                            "subject",
                            "scope",
                            "conditions",
                            "exceptions",
                            "numbers",
                            "scoring",
                            "consequence",
                            "submission",
                        )
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


def _structure_payload(artifacts, outline, units, *, outline_phase=False):
    from docreader.tender.authoring_pipeline import _constraint_context
    from docreader.tender.authoring_templates import (
        load_template_part,
        template_part_key,
    )

    from docreader.tender.authoring_excerpts import load_source_excerpt, source_excerpt_key

    if outline_phase:
        units = []
    supports, part_ids, targets, gaps, task_ids = [], {}, {}, [], {}
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

    def register(entries):
        for chapter in entries:
            for target in chapter.get("targets", []):
                targets.setdefault(target["target_id"], f"{target_prefix}{len(targets) + 1}")
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
                    except (OSError, ValueError, KeyError):
                        gaps.append(
                            {
                                "code": "structure_template_unavailable",
                                "detail": "本实例的实际格式暂不可读，结构核对保留来源缺口。",
                                "chapter_id": chapter["chapter_id"],
                                "target_id": target["target_id"],
                            }
                        )
                selected.append(
                    {
                        "target_id": identity,
                        "kind": target["kind"],
                        "purpose": target["purpose"],
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
                if target.get("source_excerpts"):
                    selected[-1]["source_support_ids"] = source_supports(target["source_excerpts"],
                        chapter["chapter_id"], target["target_id"])
                if target.get("response_table"):
                    selected[-1]["response_rows"] = [{"row_id": f"q{ordinal}",
                        "source_support_ids": source_supports(row["source_excerpts"],
                            chapter["chapter_id"], target["target_id"])}
                        for ordinal, row in enumerate(target["response_table"]["rows"], 1)]
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
    return {
        "grounding_policy": AUTHORING_GROUNDING_POLICY,
        "operation": "structure",
        **({"phase": "outline"} if outline_phase else {}),
        "chapters": tree,
        "sections": sections,
        "supports": supports,
        "units": summaries,
        "rule_dispositions": dispositions,
        "input_issues": [*context.availability_issues, *disposition_gaps],
        **submission,
    }, gaps


async def review_structure(artifacts, outline, units, budget, execute, *, outline_phase=False):
    """Return findings/gaps; the durable executor owns the existing retry allowance.

    Inspect final adopted instances once, including manual or unfilled forms.
    Do not send enterprise prose or turn structure feedback into another rewrite.
    """
    if not outline.get("chapters"):
        return [], []
    if outline_phase:
        units = []
        payload, gaps = _structure_payload(artifacts, outline, units, outline_phase=True)
    else:
        payload, gaps = _structure_payload(artifacts, outline, units)
    task_ids = {unit["task_id"] for unit in units}

    def validate(raw):
        try:
            value = AuthoringReviewResponse.model_validate(raw).model_dump()
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
        return value

    prompt = budget.prompts.get("authoring_structure")
    if not isinstance(prompt, str):
        from docreader.tender.document_execution import load_prompts

        prompt = load_prompts()["authoring_structure"]
    request = build_request(budget.model, prompt, payload, task="authoring_structure")
    try:
        from docreader.tender.authoring_pipeline import _replay

        hit, value = await _replay(execute, "authoring_structure", request, validate)
        if hit:
            return value["findings"], gaps
        try:
            budget.require_request(request, "authoring_structure")
        except ModelBudgetError:
            from docreader.tender.authoring_view import compact_structure_payload

            request = build_request(
                budget.model,
                prompt + _COMPACT_VIEW_PROMPT,
                compact_structure_payload(payload),
                task="authoring_structure",
            )
            hit, value = await _replay(execute, "authoring_structure", request, validate)
            if hit:
                return value["findings"], gaps
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
    return value["findings"], gaps
