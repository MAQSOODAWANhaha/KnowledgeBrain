"""One bounded, evidence-preserving revision after the first content review."""

from __future__ import annotations

import asyncio
from copy import deepcopy
from importlib.resources import files

from docreader.llm.request import ModelBudgetError, ModelWorkUnitExhausted, build_request


def _source_key(reference):
    if "source_ref" in reference:
        from docreader.tender.authoring_excerpts import source_excerpt_key
        return source_excerpt_key(reference)
    return reference["source_index_ref"]["sha256"], reference["unit_id"]


def _previous_content(task, unit, payload, sources, support_bindings):
    """Project persistent evidence through this request's host-owned support map."""
    by_source = {_source_key(reference): identity for identity, reference in sources.items()}
    targets = {target["target_id"]: view["target_id"]
               for target, view in zip(task["targets"], payload.get("targets", []))}
    available = {support["support_id"]: support for support in payload.get("supports", [])}
    materials = {binding["material_id"]: identity for identity, binding in support_bindings.items()
                 if binding["origin"] == "knowledge" and identity in available}
    source_supports = {_source_key(binding.get("source_excerpt") or binding["source_ref"]): identity
                       for identity, binding in support_bindings.items()
                       if binding["origin"] == "tender" and identity in available}

    def project(value, content_id, *, include_targets=True):
        precise = value.get("source_excerpts", [])
        precise_units = {_source_key(reference["source_ref"]) for reference in precise}
        references = [*precise, *(reference for reference in value.get("source_refs", [])
                                 if _source_key(reference) not in precise_units)]
        if (any(identity not in targets for identity in value.get("target_ids", []))
                or any(identity not in materials for identity in value.get("material_ids", []))
                or any(_source_key(reference) not in source_supports for reference in references)):
            return None
        support_ids = [materials[identity] for identity in value.get("material_ids", [])]
        support_ids.extend(source_supports[_source_key(reference)] for reference in references)
        projected = {**{key: deepcopy(entry) for key, entry in value.items()
                        if key not in {"material_ids", "source_refs", "source_excerpts", "target_ids"}},
                     "content_id": content_id, "support_ids": list(dict.fromkeys(support_ids))}
        if include_targets:
            projected["target_ids"] = [targets[identity] for identity in value.get("target_ids", [])]
        return projected

    blocks = []
    for ordinal, block in enumerate(unit.get("blocks", []), 1):
        projected = project(block, f"b{ordinal}")
        if projected is None:
            return None
        blocks.append(projected)
    cells = []
    for ordinal, fill in enumerate(unit.get("cell_fills", []), 1):
        source_id = by_source.get(_source_key(fill["source_ref"]))
        target_views = [view for target, view in zip(task["targets"], payload.get("targets", []))
                        if target["target_id"] in fill.get("target_ids", [])]
        if any("template_support_ids" in view for view in payload.get("targets", [])):
            permitted = [identity for identity, reference in sources.items()
                         if _source_key(reference) == _source_key(fill["source_ref"])
                         and target_views
                         and all(identity in view.get("template_support_ids", []) for view in target_views)]
            source_id = permitted[0] if permitted else None
        projected = project({key: value for key, value in fill.items() if key != "source_ref"}, f"f{ordinal}")
        if source_id is None or projected is None:
            return None
        cells.append({"source_id": source_id, **projected})
    row_aliases = {row["row_id"]: f"q{ordinal}" for ordinal, row in enumerate(
        (row for target in task["targets"] for row in (target.get("response_table") or {}).get("rows", [])), 1)}
    visible_rows = {row["row_id"] for row in payload.get("response_rows", [])}
    rows = []
    for ordinal, fill in enumerate(unit.get("row_fills", []), 1):
        row_id = row_aliases.get(fill["row_id"])
        projected = project(fill, f"r{ordinal}", include_targets=False)
        if row_id not in visible_rows or projected is None:
            return None
        rows.append({**projected, "row_id": row_id})
    if any(attachment["material_id"] not in materials
           or not available[materials[attachment["material_id"]]].get("attachment")
           or not available[materials[attachment["material_id"]]].get("text", "").strip()
           for attachment in unit.get("attachments", [])):
        return None
    return {"blocks": blocks, "cell_fills": cells, "row_fills": rows,
            "attachments": [{"content_id": f"a{ordinal}", "attachment_id": materials[attachment["material_id"]],
                             "title": attachment["title"]}
                            for ordinal, attachment in enumerate(unit.get("attachments", []), 1)],
            "gaps": list(unit.get("gaps", []))}


def _has_allowed_support(task, payload):
    supports = {support["support_id"]: support for support in payload.get("supports", [])}
    for target, view in zip(task["targets"], payload.get("targets", [])):
        if target.get("manual_fill", False):
            continue
        for identity in view.get("support_ids", []):
            support = supports.get(identity)
            if (support and support["origin"] in target.get("support_origins", ["knowledge"])
                    and (support.get("text", "").strip()
                         or support["origin"] == "tender" and support.get("grid"))):
                return True
    return False


def _has_content(unit):
    return bool(any(block.get("text", "").strip() or any(h.strip() for h in block.get("headers", []))
                    or any(any(cell.strip() for cell in row) for row in block.get("rows", []))
                    for block in unit.get("blocks", []))
                or any(fill["text"].strip() for fill in unit.get("cell_fills", []))
                or any(fill["text"].strip() for fill in unit.get("row_fills", []))
                or unit.get("attachments"))


async def revise_units(artifacts, budget, execute, tasks, units, findings, make_payload, bind_response):
    """Produce at most one replacement; the caller owns safe final adoption.

    make_payload(task) returns (writing_payload, source_id_to_SourceUnitRef,
    host_support_bindings). Bindings never enter the model request.
    bind_response(task, raw) binds a WritingResponse to a host-owned WritingUnit.
    execute owns the existing paid retry/correction limit; this function never
    loops a failed request or retrieves additional knowledge. Cancellation,
    lease loss and storage failures propagate to the enclosing execution.
    """
    by_id = {task["task_id"]: task for task in tasks}
    revised, superseded, diagnostics = deepcopy(units), [], []

    def diagnostic(code, detail, unit):
        value = {"code": code, "detail": detail, "task_id": unit["task_id"], "chapter_id": unit["chapter_id"]}
        if len(unit.get("target_ids", [])) == 1:
            value["target_id"] = unit["target_ids"][0]
        diagnostics.append(value)

    for ordinal, unit in enumerate(units):
        relevant = [finding for finding in findings if unit["task_id"] in finding.get("task_ids", [])]
        if unit.get("error_type") or not any(finding.get("can_revise") is True for finding in relevant):
            continue
        task = by_id.get(unit["task_id"])
        if task is None:
            diagnostic("authoring_revision_unbound", "改稿任务无法绑定当前写作计划，保留已有正文。", unit)
            continue
        try:
            payload, sources, support_bindings = await asyncio.to_thread(make_payload, task)
            if not _has_allowed_support(task, payload):
                continue
            previous = _previous_content(task, unit, payload, sources, support_bindings)
            if previous is None:
                diagnostic("authoring_revision_source_unavailable", "原稿的模板或真实附件无法绑定当前资料，保留原稿待补读。", unit)
                continue
            content = {**deepcopy(payload), "previous_content": previous,
                       "review_findings": [{key: finding[key] for key in ("code", "detail", "suggestion", "can_revise", "reject_content", "content_ids")
                                            if key in finding} for finding in relevant]}
            prompt = budget.prompts.get("authoring_write") or files("docreader.tender.prompts").joinpath(
                "authoring_write.md").read_text("utf-8")
            request = build_request(budget.model, prompt, content, task="authoring_write")
            from docreader.tender.authoring_pipeline import _replay
            validate = lambda raw: bind_response(task, raw)
            hit, replacement = await _replay(execute, "authoring_write", request, validate)
            if not hit:
                budget.require_request(request, "authoring_write")
                replacement = await execute("authoring_write", request, validate)
        except (ModelWorkUnitExhausted, ModelBudgetError) as error:
            diagnostic("authoring_revision_unavailable", f"本次改稿未完成（{type(error).__name__}），由采用流程保留安全稿。", unit)
            continue
        if (replacement.get("task_id") != unit["task_id"] or replacement.get("chapter_id") != unit["chapter_id"]
                or replacement.get("target_ids") != unit.get("target_ids")):
            diagnostic("authoring_revision_unbound", "改稿未绑定原响应位置，保留已有正文。", unit)
            continue
        if replacement.get("status") != "generated" or not _has_content(replacement):
            diagnostic("authoring_revision_empty", "改稿没有形成可替换的实际内容，由采用流程保留安全稿。", unit)
            continue
        if replacement == unit:
            continue
        superseded.append(artifacts.put_json(unit))
        revised[ordinal] = replacement
    return revised, superseded, diagnostics
