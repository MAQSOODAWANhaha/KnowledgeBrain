"""Bounded model-driven Level-2/Level-3 sub-outline expansion for open proposal chapters.

Rules and five questions: workflow.md「新链格式补位与事项归属」; contracts: field-dictionary.md.
"""

from __future__ import annotations

import asyncio
from copy import deepcopy
import json
import re
from typing import Annotated, Any

from pydantic import Field, ValidationError

from docreader.llm.request import ModelBudgetError, ModelContractError, ModelWorkUnitExhausted
from docreader.tender.schema import Contract, Text
from docreader.tender.status import AuthoringTask


PROPOSAL_OUTLINE_OPERATION = "plan_proposal_outline_v1"
PROPOSAL_TREE_OPERATION = "plan_proposal_tree_v1"
PROPOSAL_MIN_RULES = 6
PROPOSAL_MAX_DEPTH = 2
PROPOSAL_INPUT_FRACTION = 0.5
PROPOSAL_PENDING_CODE = "outline_proposal_structure_pending"

_HEADING_CHAPTER = re.compile(r"^第[0-9０-９一二三四五六七八九十百千]+[章节部分编]\s+\S+")
_HEADING_CN = re.compile(r"^[一二三四五六七八九十百千]+\s*[、．.]\s*\S+")
_HEADING_PAREN = re.compile(r"^[（(][一二三四五六七八九十百千]+[）)]\s*\S+")
_HEADING_DOTTED = re.compile(r"^(?P<num>\d+(?:\.\d+){1,4})\s+(?P<rest>[^\d\s].*)$")
_HEADING_BAD_END = ("。", "；", "：", "，", "、", "？", "！")


class ProposalSection(Contract):
    id: Text
    parent_id: Text | None = None
    title: Text


class ProposalOutlineResponse(Contract):
    sections: list[ProposalSection] = Field(min_length=2)
    assignments: list[Annotated[list[Text | None], Field(min_length=2, max_length=2)]]


class ProposalTreeOnlyResponse(Contract):
    sections: list[ProposalSection] = Field(min_length=2)


def _issue(detail: str, path: tuple[Any, ...] = ()) -> dict[str, Any]:
    return {"path": list(path), "code": "proposal_outline_invalid", "detail": detail}


def _heading_hint(line: str) -> tuple[int, str] | None:
    text = line.strip()
    if not text or len(text) > 40 or text.endswith(_HEADING_BAD_END) or "。" in text or "；" in text:
        return None
    if _HEADING_CHAPTER.match(text):
        return 0, text
    if _HEADING_CN.match(text):
        return 1, text
    if _HEADING_PAREN.match(text):
        return 2, text
    dotted = _HEADING_DOTTED.match(text)
    if dotted:
        rest = dotted.group("rest").strip()
        if len(rest) <= 32 and not any(mark in rest for mark in ("。", "；", "，", "：")):
            return len(dotted.group("num").split(".")) + 1, text
    return None


def _source_context_maps(artifacts, authoring_input):
    """Read-only lookup from (artifact_sha, item_id) -> item summary & source section path hints."""
    if artifacts is None or not isinstance(authoring_input, dict):
        return {}, {}
    range_to_unit: dict[str, str] = {}
    unit_paths: dict[str, tuple[str, ...]] = {}
    frozen_ref = authoring_input.get("frozen_ref")
    if frozen_ref:
        try:
            frozen = artifacts.read_json(frozen_ref)
        except Exception:
            frozen = {}
        for document in frozen.get("documents") or []:
            index_ref = document.get("source_index_ref")
            raw_ref = document.get("raw_ref")
            if not index_ref or not raw_ref:
                continue
            try:
                index = artifacts.read_json(index_ref)
                raw = artifacts.read_json(raw_ref)
            except Exception:
                continue
            for row in index.get("ranges") or []:
                rid, uid = row.get("range_id"), row.get("unit_id")
                if rid and uid:
                    range_to_unit[rid] = uid
            stack: list[tuple[int, str]] = []
            for unit in raw.get("units") or []:
                if unit.get("kind") != "table_region":
                    for line in (unit.get("text") or "").splitlines():
                        parsed = _heading_hint(line)
                        if parsed is None:
                            continue
                        depth, label = parsed
                        while stack and stack[-1][0] >= depth:
                            stack.pop()
                        stack.append((depth, label))
                uid = unit.get("unit_id")
                if uid:
                    unit_paths[uid] = tuple(label for _d, label in stack)
    items: dict[tuple[str, str], dict[str, Any]] = {}
    refs = list(authoring_input.get("extract_refs") or [])
    if authoring_input.get("items_ref"):
        refs.append(authoring_input["items_ref"])
    for ref in refs:
        sha = (ref or {}).get("sha256")
        if not sha:
            continue
        try:
            data = artifacts.read_json(ref)
        except Exception:
            continue
        for item in data.get("items") or []:
            item_id = item.get("item_id")
            if not item_id:
                continue
            evidence = item.get("evidence") or []
            path: tuple[str, ...] = ()
            if evidence:
                rid = evidence[0].get("range_id")
                uid = range_to_unit.get(rid) if rid else None
                if uid:
                    path = unit_paths.get(uid, ())
            items[(sha, item_id)] = {
                "kind": item.get("kind") or "requirement",
                "text": str(item.get("text") or "").strip(),
                "scoring": item.get("scoring"),
                "section_path": list(path),
            }
    return items, unit_paths


def decode_proposal_sections(
    raw: Any,
    *,
    model_cls: type[ProposalTreeOnlyResponse] | type[ProposalOutlineResponse] = ProposalTreeOnlyResponse,
) -> list[dict[str, Any]]:
    try:
        parsed = model_cls.model_validate(raw).model_dump()
    except ValidationError as error:
        raise ModelContractError([
            {"path": list(item["loc"]), "code": item["type"], "detail": item["msg"]}
            for item in error.errors(include_input=False)
        ]) from None
    sections: list[dict[str, Any]] = []
    by_id: dict[str, dict[str, Any]] = {}
    for index, section in enumerate(parsed["sections"]):
        sid = section["id"].strip()
        title = section["title"].strip()
        parent_id = section["parent_id"].strip() if isinstance(section.get("parent_id"), str) and section["parent_id"].strip() else None
        if parent_id is not None and parent_id.lower() == "null":
            parent_id = None
        if not sid or sid in by_id:
            raise ModelContractError([_issue("子章节 id 必须非空且唯一。", ("sections", index, "id"))])
        if not title:
            raise ModelContractError([_issue("子章节标题不能为空。", ("sections", index, "title"))])
        cleaned = {"id": sid, "parent_id": parent_id, "title": title}
        by_id[sid] = cleaned
        sections.append(cleaned)
    for index, section in enumerate(sections):
        depth, current, seen = 1, section["parent_id"], {section["id"]}
        while current is not None:
            if current not in by_id:
                raise ModelContractError([_issue("parent_id 必须指向本批已定义的子章节 id。", ("sections", index, "parent_id"))])
            if current in seen:
                raise ModelContractError([_issue("子章节父子关系不能成环。", ("sections", index, "parent_id"))])
            seen.add(current)
            depth += 1
            if depth > PROPOSAL_MAX_DEPTH:
                raise ModelContractError([_issue(f"方案子章节相对父章最多展开 {PROPOSAL_MAX_DEPTH} 级。", ("sections", index, "parent_id"))])
            current = by_id[current]["parent_id"]
    return sections


def decode_proposal_outline(raw: Any, rule_ids: list[str]) -> tuple[list[dict[str, Any]], dict[str, str | None]]:
    sections = decode_proposal_sections(raw, model_cls=ProposalOutlineResponse)
    valid_section_ids = {section["id"] for section in sections}
    parsed = ProposalOutlineResponse.model_validate(raw).model_dump()
    seen: dict[str, str | None] = {}
    expected = set(rule_ids)
    for index, pair in enumerate(parsed["assignments"]):
        rid, target = pair
        if rid not in expected or rid in seen:
            raise ModelContractError([_issue("本批每条要求 id 恰好出现一次，不能包含本批以外的 id。", ("assignments", index))])
        if isinstance(target, str) and target.strip().lower() == "null":
            target = None
        if target is not None and target not in valid_section_ids:
            raise ModelContractError([_issue("分配目标只能是 sections 中的 id 或 null。", ("assignments", index))])
        seen[rid] = target
    if seen.keys() != expected:
        raise ModelContractError([_issue("本批每条要求都必须分配到子章节或 null。", ("assignments",))])
    return sections, seen


def _proposal_payload(chapter_title: str, instruction: str, keyed_rules: dict[str, dict[str, Any]], *, tree_only: bool = False) -> dict[str, Any]:
    headings: list[str] = []
    seen_headings: set[str] = set()
    req_rows: list[dict[str, Any]] = []
    for rid, info in keyed_rules.items():
        path = info.get("section_path") or []
        if path:
            joined = " > ".join(path)
            if joined not in seen_headings:
                seen_headings.add(joined)
                headings.append(joined)
        if not tree_only:
            row: dict[str, Any] = {"id": rid, "kind": info["kind"], "text": info["text"]}
            if info.get("scoring"):
                row["scoring"] = info["scoring"]
            if path:
                row["section_path"] = path
            req_rows.append(row)
    if tree_only:
        # Bounded summary for oversized chapters: scoring rules + unique topic lines.
        for rid, info in keyed_rules.items():
            if info.get("scoring") or len(req_rows) < 60:
                row = {"id": rid, "kind": info["kind"], "text": info["text"][:120]}
                if info.get("scoring"):
                    row["scoring"] = info["scoring"]
                req_rows.append(row)
    return {
        "operation": PROPOSAL_TREE_OPERATION if tree_only else PROPOSAL_OUTLINE_OPERATION,
        "chapter_title": chapter_title,
        "instruction": instruction,
        "source_headings": headings,
        "requirements": req_rows,
    }


def _fits_half_input(budget, payload: dict[str, Any], fits) -> bool:
    if not fits(payload):
        return False
    from docreader.tender.authoring_pipeline import _request

    measured = budget.require_request(
        _request(budget, AuthoringTask.PROPOSAL_OUTLINE, payload), AuthoringTask.PROPOSAL_OUTLINE
    )
    return measured["input_tokens"] <= int(measured["effective_input_budget_tokens"] * PROPOSAL_INPUT_FRACTION)


def _output_fits(budget, rule_ids: list[str]) -> bool:
    bound = len(json.dumps({"sections": [{"id": "s99_99", "parent_id": "s99", "title": "示例标题" * 3}] * 16,
                            "assignments": [[rid, "s99_99"] for rid in rule_ids]}, ensure_ascii=False, separators=(",", ":")))
    return bound < budget.output_reserve_for(AuthoringTask.PROPOSAL_OUTLINE, tokenized=budget.tokenizer is not None)


def _materialize_sections(
    parent_chapter_id: str,
    form_target_id: str,
    sections: list[dict[str, Any]],
    assignments: dict[str, str | None],
    rule_refs_by_id: dict[str, dict[str, Any]],
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    by_section: dict[str, list[dict[str, Any]]] = {section["id"]: [] for section in sections}
    leftover: list[dict[str, Any]] = []
    for rid, rule_ref in rule_refs_by_id.items():
        sid = assignments.get(rid)
        if sid in by_section:
            by_section[sid].append(deepcopy(rule_ref))
        else:
            leftover.append(deepcopy(rule_ref))

    children_by_parent: dict[str | None, list[dict[str, Any]]] = {}
    for section in sections:
        children_by_parent.setdefault(section["parent_id"], []).append(section)

    def build(parent_sid: str | None, id_prefix: str, depth: int) -> list[dict[str, Any]]:
        built: list[dict[str, Any]] = []
        for idx, section in enumerate(children_by_parent.get(parent_sid, []), start=1):
            chapter_id = f"{id_prefix}-p{idx}" if depth == 1 else f"{id_prefix}.{idx}"
            sub_chapters = build(section["id"], chapter_id, depth + 1)
            direct_rules = by_section.get(section["id"], [])
            targets: list[dict[str, Any]] = []
            if direct_rules or not sub_chapters:
                targets.append({
                    "target_id": f"{chapter_id}:target-1",
                    "kind": "narrative",
                    "purpose": f"依据招标文件要求编制「{section['title']}」。",
                    "rule_refs": direct_rules,
                    "source_refs": [],
                    "manual_fill": False,
                    "support_origins": ["knowledge"],
                    "related_target_ids": [form_target_id],
                })
            built.append({
                "chapter_id": chapter_id,
                "title": section["title"],
                "targets": targets,
                "children": sub_chapters,
                "is_volume": False,
                "presentation_role": "body",
            })
        return built

    return build(None, parent_chapter_id, 1), leftover


def _find_proposal_candidates(chapters: list[dict[str, Any]]) -> list[tuple[dict[str, Any], dict[str, Any], dict[str, Any]]]:
    found: list[tuple[dict[str, Any], dict[str, Any], dict[str, Any]]] = []
    for chapter in chapters:
        targets = chapter.get("targets") or []
        form = next((t for t in targets if t.get("kind") == "form" and t.get("manual_fill") and t.get("support_origins") == ["tender"]), None)
        answer = next((t for t in targets if t.get("kind") == "narrative" and not t.get("manual_fill")
                       and t.get("support_origins") == ["knowledge"] and str(t.get("target_id", "")).endswith(":answer")
                       and len(t.get("rule_refs") or []) >= PROPOSAL_MIN_RULES), None)
        already_expanded = any(
            str(child.get("chapter_id", "")).startswith(f"{chapter['chapter_id']}-p")
            for child in chapter.get("children") or []
        )
        if form is not None and answer is not None and not already_expanded:
            found.append((chapter, form, answer))
        found.extend(_find_proposal_candidates(chapter.get("children") or []))
    return found


async def _assign_rules_in_batches(
    sections: list[dict[str, Any]],
    rule_refs_by_id: dict[str, dict[str, Any]],
    keyed_rules: dict[str, dict[str, Any]],
    budget,
    execute,
    ask,
    fits_placement,
) -> dict[str, str | None]:
    """Oversized fallback: assign rules into the generated section tree using item_placement batching."""
    from docreader.tender.item_placement import place_items

    synthetic_chapters, _ = _materialize_sections("proposal", "form", sections, {}, {})
    synthetic_outline = {"chapters": synthetic_chapters, "issues": []}
    target_to_sid: dict[str, str] = {}

    def map_targets(chapters_list, parent_sid=None, depth=1):
        siblings = [s for s in sections if s["parent_id"] == parent_sid]
        for ch, sec in zip(chapters_list, siblings):
            for t in ch.get("targets") or []:
                target_to_sid[t["target_id"]] = sec["id"]
            map_targets(ch.get("children") or [], sec["id"], depth + 1)

    map_targets(synthetic_chapters)
    items = [
        {
            "rule_ref": deepcopy(rule_refs_by_id[rid]),
            "semantic": {
                "kind": keyed_rules[rid]["kind"],
                "text": keyed_rules[rid]["text"],
                **({"scoring": keyed_rules[rid]["scoring"]} if keyed_rules[rid].get("scoring") else {}),
            },
            "evidence": [],
        }
        for rid in rule_refs_by_id
    ]
    placed_outline, _stats = await place_items(synthetic_outline, items, budget, execute, ask, fits_placement)

    ref_to_rid = {json.dumps(ref, sort_keys=True, ensure_ascii=False): rid for rid, ref in rule_refs_by_id.items()}
    assignments: dict[str, str | None] = {rid: None for rid in rule_refs_by_id}

    def collect(chapters_list):
        for ch in chapters_list:
            for t in ch.get("targets") or []:
                sid = target_to_sid.get(t["target_id"])
                for ref in t.get("rule_refs") or []:
                    rid = ref_to_rid.get(json.dumps(ref, sort_keys=True, ensure_ascii=False))
                    if rid is not None and sid is not None:
                        assignments[rid] = sid
            collect(ch.get("children") or [])

    collect(placed_outline["chapters"])
    return assignments


async def expand_proposal_outlines(
    outline: dict[str, Any],
    artifacts,
    budget,
    execute,
    ask,
    fits_proposal,
    fits_placement,
) -> dict[str, Any]:
    """Expand open proposal chapters into Level-2/Level-3 sub-chapters grounded in tender rules."""
    if execute is None:
        return outline
    candidates = _find_proposal_candidates(outline.get("chapters") or [])
    if not candidates:
        return outline

    from docreader.tender.authoring_pipeline import _plan

    await _plan(execute, AuthoringTask.PROPOSAL_OUTLINE, len(candidates), "proposal-outline")
    items_map, _ = _source_context_maps(artifacts, outline.get("authoring_input"))

    async def expand_one(chapter: dict[str, Any], form: dict[str, Any], answer: dict[str, Any]):
        rules = list(answer.get("rule_refs") or [])
        rule_refs_by_id: dict[str, dict[str, Any]] = {}
        keyed_rules: dict[str, dict[str, Any]] = {}
        for idx, ref in enumerate(rules, start=1):
            rid = f"r{idx}"
            rule_refs_by_id[rid] = ref
            item_ref = ref.get("item_ref") or {}
            sha = (item_ref.get("artifact_ref") or {}).get("sha256")
            iid = item_ref.get("item_id")
            info = items_map.get((sha, iid)) if sha and iid else None
            keyed_rules[rid] = info or {"kind": "requirement", "text": str(iid or rid), "scoring": None, "section_path": []}

        rule_ids = list(rule_refs_by_id)
        instruction = (form.get("purpose") or "").strip()
        full_payload = _proposal_payload(chapter["title"], instruction, keyed_rules, tree_only=False)
        try:
            if _output_fits(budget, rule_ids) and _fits_half_input(budget, full_payload, fits_proposal):
                sections, assignments = await ask(
                    execute, budget, AuthoringTask.PROPOSAL_OUTLINE, full_payload,
                    lambda raw, ids=rule_ids: decode_proposal_outline(raw, ids),
                )
            else:
                tree_payload = _proposal_payload(chapter["title"], instruction, keyed_rules, tree_only=True)
                if not fits_proposal(tree_payload):
                    tree_payload["requirements"] = tree_payload["requirements"][:20]
                sections = await ask(
                    execute, budget, AuthoringTask.PROPOSAL_OUTLINE, tree_payload,
                    lambda raw: decode_proposal_sections(raw, model_cls=ProposalTreeOnlyResponse),
                )
                assignments = await _assign_rules_in_batches(
                    sections, rule_refs_by_id, keyed_rules, budget, execute, ask, fits_placement
                )
        except (ModelWorkUnitExhausted, ModelBudgetError):
            return chapter, form, answer, None, None

        new_children, leftover = _materialize_sections(
            chapter["chapter_id"], form["target_id"], sections, assignments, rule_refs_by_id
        )
        return chapter, form, answer, new_children, leftover

    results = await asyncio.gather(*(expand_one(chapter, form, answer) for chapter, form, answer in candidates))
    for chapter, _form, answer, new_children, leftover in results:
        if new_children is None:
            outline.setdefault("issues", []).append({
                "code": PROPOSAL_PENDING_CODE,
                "detail": f"「{chapter['title']}」未能生成二级/三级方案子目录（请求未采纳或超出预算），保留单目标应答位待人工编排。",
                "rule_refs": [],
            })
            continue
        chapter["children"] = [*new_children, *(chapter.get("children") or [])]
        if leftover:
            answer["rule_refs"] = leftover
        else:
            chapter["targets"] = [t for t in chapter["targets"] if t is not answer]
    return outline
