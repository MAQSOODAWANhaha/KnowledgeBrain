"""Host ledger for retrieval plans and final coverage. No extra model call."""

from __future__ import annotations

import hashlib
import json
import re

from pydantic import ValidationError

from docreader.tender.schema import SourceExcerptRef


QUERY_BYTE_LIMIT = 20000
_SETTLED = {"manual", "constraint", "submission", "excluded"}
_QUERY_GAP = {
    "knowledge_query_missing", "knowledge_query_too_large", "knowledge_unconfigured",
    "knowledge_unavailable", "knowledge_no_hits", "knowledge_hit_invalid",
    "knowledge_hit_out_of_scope", "knowledge_original_unavailable",
    "knowledge_candidates_not_loaded", "knowledge_partial_results",
}
_NAMED_CHAPTER = r'“第[0-9０-９一二三四五六七八九十百千]+章[^“”\n]+”'
_PROPOSAL_INSTRUCTION = re.compile(
    rf'投标人(?:应当|应|须)根据{_NAMED_CHAPTER}(?:[、，]{_NAMED_CHAPTER})*(?:等)?要求编制(?:相关)?内容[。.]?'
)


def _json(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def _digest(value, prefix):
    return prefix + hashlib.sha256(_json(value).encode()).hexdigest()[:24]


def canonical_rule_ref(value):
    if not isinstance(value, dict):
        return value
    return {key: item for key, item in value.items()
            if key not in {"decision_id", "rule_index"} or item is not None}


def rule_ref_key(value):
    if value is None:
        return None
    return _json(canonical_rule_ref(value))


def coverage_identity(rule_ref, target_id, row_id):
    return _digest({"rule_ref": canonical_rule_ref(rule_ref), "target_id": target_id, "row_id": row_id}, "c-")


def query_identity(query, scope, include_original):
    return _digest({"query": query, "scope": scope, "include_original": include_original}, "q-")


def _canonical(value):
    if value is None:
        return None
    return _json(value)


def _walk(chapters, external=False):
    for chapter in chapters or []:
        external_note = external or chapter.get("presentation_role") == "submission_note"
        for target in chapter.get("targets", []):
            yield target, external_note
        yield from _walk(chapter.get("children", []), external_note)


def _safe_excerpts(values):
    result = []
    for value in values or []:
        try:
            result.append(SourceExcerptRef.model_validate(value).model_dump())
        except ValidationError:
            continue
    return result


def _resolution(rule_ref, target, constraints, excluded_rules, excluded_targets, external=False):
    if target is not None and target.get("manual_fill"):
        return "manual"
    if target is not None and target.get("target_id") in excluded_targets:
        return "excluded"
    key = rule_ref_key(rule_ref)
    if key and key in excluded_rules:
        return "excluded"
    if external:
        return "submission"
    if key and key in constraints and target is None:
        return "constraint"
    return "content"


def build_ledger(outline, requirements=None):
    """One entry per explicit rule position or response row. Not a cartesian product."""
    requirements = requirements or {}
    excluded_rules, excluded_targets = set(), set()
    for issue in outline.get("issues", []):
        if issue.get("code") in {"outline_rule_excluded", "outline_target_excluded"}:
            excluded_rules.update(rule_ref_key(ref) for ref in issue.get("rule_refs", []))
            if issue.get("target_id"):
                excluded_targets.add(issue["target_id"])
    constraints = {rule_ref_key(ref) for ref in outline.get("constraint_rule_refs", [])}
    entries, seen, placed = [], set(), set()

    def add(rule_ref, target, row, resolution):
        rule_ref = canonical_rule_ref(rule_ref)
        target_id = None if target is None else target.get("target_id")
        row_id = None if row is None else row.get("row_id")
        if rule_ref is None and not target_id and not row_id:
            return
        ref_key = rule_ref_key(rule_ref)
        key = (ref_key, target_id, row_id)
        if key in seen:
            return
        seen.add(key)
        if ref_key is not None:
            placed.add(ref_key)
        excerpts = ([*(row or target or {}).get("context_source_excerpts", []),
                     *(row or target or {}).get("source_excerpts", [])] if row or target else [])
        status = "not_required" if resolution in _SETTLED else "unreviewed"
        entries.append({
            "coverage_id": coverage_identity(rule_ref, target_id, row_id),
            "rule_ref": rule_ref, "target_id": target_id, "row_id": row_id,
            "source_excerpts": _safe_excerpts(excerpts), "resolution": resolution,
            "query_ids": [], "material_ids": [], "content_refs": [],
            "assessment": None, "review_status": status, "gap_codes": [],
        })

    def add_target(target, resolve):
        rows = ((target.get("response_table") or {}).get("rows") or [])
        if rows:
            row_refs = set()
            for row in rows:
                refs = row.get("rule_refs") or [None]
                for ref in refs:
                    row_refs.add(rule_ref_key(ref))
                    add(ref, target, row, resolve(ref))
            for ref in target.get("rule_refs", []):
                if rule_ref_key(ref) not in row_refs:
                    add(ref, target, None, resolve(ref))
            return
        refs = target.get("rule_refs") or []
        if refs:
            for ref in refs:
                add(ref, target, None, resolve(ref))
        elif target.get("manual_fill") or resolve(None) in {"pending", "submission"} or "knowledge" in (target.get("support_origins") or ["knowledge"]):
            add(None, target, None, resolve(None))

    assigned_targets = set()
    for target, external in _walk(outline.get("chapters", [])):
        assigned_targets.add(target["target_id"])
        add_target(target, lambda reference: _resolution(
            reference, target, constraints, excluded_rules, excluded_targets, external))
    for target in outline.get("unplaced_targets", []):
        if target["target_id"] not in assigned_targets:
            add_target(target, lambda _reference: "pending")
    for ref in outline.get("constraint_rule_refs", []):
        if rule_ref_key(ref) not in placed:
            add(ref, None, None, "constraint")
    for issue in outline.get("issues", []):
        if issue.get("code") not in {"outline_target_excluded", "outline_rule_excluded"}:
            continue
        for ref in issue.get("rule_refs", []):
            if rule_ref_key(ref) not in placed:
                add(ref, None, None, "excluded")
    for row in requirements.get("rules", []):
        ref = row.get("rule_ref")
        key = rule_ref_key(ref)
        if ref is not None and key not in placed and key not in constraints and key not in excluded_rules:
            add(ref, None, None, "pending")
    return entries


def _row_ranges(row):
    return {selected.get("range_id") for ref in row.get("source_excerpts", [])
            for selected in ref.get("selectors", []) if selected.get("range_id")}


def _row_sources(row):
    return {_canonical(ref.get("source_ref", ref)) for ref in row.get("source_excerpts", [])}


def _exact_intents(intents, ranges):
    matched, covered = [], set()
    for intent in intents:
        intent_ranges = set(intent.get("range_ids") or [])
        if intent_ranges and ranges and intent_ranges <= ranges:
            matched.append(intent)
            covered.update(intent_ranges)
    return matched if covered == ranges else []


def _row_intents(intents, row, context_excerpts=()):
    selected = {"source_excerpts": [*context_excerpts, *row.get("context_source_excerpts", []), *row.get("source_excerpts", [])]}
    ranges, sources = _row_ranges(selected), _row_sources(selected)
    if ranges:
        return _exact_intents(intents, ranges)
    return [intent for intent in intents if not intent.get("rule_refs") and not intent.get("range_ids")
            and (intent_sources := {_canonical(ref) for ref in intent.get("source_refs", [])})
            and sources and intent_sources <= sources]


def _source_intents(intents, target):
    """Source-only positions may use an intent bound to their own sources, never a sibling rule."""
    sources = {_canonical(ref) for ref in target.get("source_refs", [])}
    ranges = _row_ranges(target)
    candidates = [intent for intent in intents if not intent.get("rule_refs")]
    if ranges:
        return _exact_intents(candidates, ranges)
    return [intent for intent in candidates if not intent.get("range_ids")
            and sources.intersection(_canonical(ref) for ref in intent.get("source_refs", []))]


def _rule_intents(intents, rule_ref):
    key = rule_ref_key(rule_ref)
    return [intent for intent in intents if any(rule_ref_key(ref) == key for ref in intent.get("rule_refs", []))]


def _row_text(row, excerpt_texts, context_excerpts=()):
    parts, seen = [], set()
    for ref in [*context_excerpts, *row.get("context_source_excerpts", []), *row.get("source_excerpts", [])]:
        key = _json(ref)
        if key in seen:
            continue
        seen.add(key)
        text = excerpt_texts.get(key, "")
        if not isinstance(text, str) or not text.strip():
            return ""
        parts.append(text)
    return "\n".join(parts)


def plan_queries(outline, tasks, entries, requirements=None, excerpt_texts=None, scope=None):
    """Supplement a missing per-rule or per-row intent once. Do not invent a second wording."""
    requirements = requirements or {}
    excerpt_texts = excerpt_texts or {}
    scope = scope or {}
    texts = {}
    for row in requirements.get("rules", []):
        text = ((row.get("rule") or {}).get("text"))
        if row.get("rule_ref") is not None and isinstance(text, str):
            texts[rule_ref_key(row["rule_ref"])] = text
    by_target = {}
    for entry in entries:
        by_target.setdefault(entry.get("target_id"), []).append(entry)
    intents = outline.get("search_intents") or []
    gaps, raw = [], []

    def gap(entry, code, detail, target):
        if code not in entry["gap_codes"]:
            entry["gap_codes"].append(code)
        item = {"code": code, "detail": detail, "target_id": target.get("target_id")}
        if item not in gaps:
            gaps.append(item)

    def consider(text, origin, group, target):
        if not isinstance(text, str) or not text.strip():
            for entry in group:
                gap(entry, "knowledge_query_missing", "本条要求或条件没有完整可读检索文本，保留完整位置，不另编问句。", target)
            return
        if len(text.encode()) > QUERY_BYTE_LIMIT:
            for entry in group:
                gap(entry, "knowledge_query_too_large", "要求超过当前知识检索入口上限，完整原要求保留，此项待补。", target)
            return
        original = target.get("kind") == "attachment"
        for entry in group:
            raw.append({"query": text, "origin": origin, "coverage_id": entry["coverage_id"],
                        "target_id": target["target_id"], "include_original": original})

    excluded = list(scope.get("exclude_content_hashes") or [])
    for task in tasks:
        for target in task["targets"]:
            origins = target.get("support_origins") or ["knowledge"]
            if target.get("manual_fill") or "knowledge" not in origins:
                continue
            if any(target.get(flag) for flag in ("pending_line", "unread_image", "attachment_applicability_pending")):
                continue
            related = [entry for entry in by_target.get(target["target_id"], []) if entry["resolution"] == "content"]
            rows = ((target.get("response_table") or {}).get("rows") or [])
            if rows:
                for row in rows:
                    group = [entry for entry in related if entry.get("row_id") == row.get("row_id")]
                    if not group:
                        continue
                    matched = _row_intents(intents, row, target["response_table"].get("context_source_excerpts", []))
                    if matched:
                        consider_intents = matched
                        for intent in consider_intents:
                            for entry in group:
                                raw.append({"query": intent["query"], "origin": "intent",
                                            "coverage_id": entry["coverage_id"], "target_id": target["target_id"],
                                            "include_original": target.get("kind") == "attachment"})
                        continue
                    consider(_row_text(row, excerpt_texts,
                        target["response_table"].get("context_source_excerpts", [])), "source_excerpt", group, target)
                related = [entry for entry in related if entry.get("row_id") is None]
            bare = [entry for entry in related if not entry.get("rule_ref")]
            if bare:
                matched = _source_intents(intents, target)
                if matched:
                    for intent in matched:
                        for entry in bare:
                            raw.append({"query": intent["query"], "origin": "intent",
                                        "coverage_id": entry["coverage_id"], "target_id": target["target_id"],
                                        "include_original": target.get("kind") == "attachment"})
                else:
                    text = _row_text(target, excerpt_texts)
                    if not str(text or "").strip() and target.get("related_target_ids"):
                        text = target.get("purpose")
                    consider(text, "source_excerpt", bare, target)
            for entry in related:
                if not entry.get("rule_ref"):
                    continue
                matched = _rule_intents(intents, entry["rule_ref"])
                if matched:
                    for intent in matched:
                        raw.append({"query": intent["query"], "origin": "intent",
                                    "coverage_id": entry["coverage_id"], "target_id": target["target_id"],
                                    "include_original": target.get("kind") == "attachment"})
                    continue
                consider(texts.get(rule_ref_key(entry["rule_ref"])), "effective_requirement", [entry], target)
    grouped = {}
    for item in raw:
        bucket = grouped.setdefault(item["query"], {"query": item["query"], "bindings": [], "targets": [],
                                                    "include_original": False})
        bucket["include_original"] = bucket["include_original"] or item["include_original"]
        binding = {"coverage_id": item["coverage_id"], "origin": item["origin"]}
        if binding not in bucket["bindings"]:
            bucket["bindings"].append(binding)
        if item["target_id"] not in bucket["targets"]:
            bucket["targets"].append(item["target_id"])
    queries, search = [], []
    for bucket in grouped.values():
        query_id = query_identity(bucket["query"], scope, bucket["include_original"])
        for binding in bucket["bindings"]:
            entry = next(item for item in entries if item["coverage_id"] == binding["coverage_id"])
            if query_id not in entry["query_ids"]:
                entry["query_ids"].append(query_id)
        queries.append({"query_id": query_id, "query": bucket["query"], "bindings": bucket["bindings"],
                        "scope": scope, "include_original": bucket["include_original"]})
        for target_id in bucket["targets"]:
            search.append({"target_id": target_id, "query": bucket["query"], "query_id": query_id,
                           "include_original": bucket["include_original"],
                           "exclude_content_hashes": list(excluded)})
    return {"queries": queries, "search": search, "gaps": gaps}


def attach_knowledge_answer_slots(outline):
    """Add a knowledge answer beside a read-only tender slot. Do not rewrite the tender text."""
    for chapter in outline.get("chapters") or []:
        _attach_chapter_answers(chapter)
    return outline


def _attach_chapter_answers(chapter):
    added = []
    for target in chapter.get("targets") or []:
        if not target.get("enterprise_response"):
            continue
        clause = target.get("purpose") or ""
        target.pop("enterprise_response", None)
        if target.get("manual_fill") is not True or target.get("support_origins") != ["tender"] or not clause.strip():
            continue
        added.append(_knowledge_answer(target, []))
    chapter.setdefault("targets", []).extend(added)
    for child in chapter.get("children") or []:
        _attach_chapter_answers(child)


def _knowledge_answer(target, rules):
    return {
        "target_id": target["target_id"] + ":answer",
        "kind": "narrative",
        "purpose": target["purpose"],
        "rule_refs": rules,
        "source_refs": [],
        "manual_fill": False,
        "support_origins": ["knowledge"],
        "related_target_ids": [target["target_id"]],
    }


def attach_w2_proposal_answers(outline):
    """Give an explicit proposal instruction a separate knowledge-backed response after placement."""
    from docreader.tender.directory_compile import _FORMAT_ITEM

    def attach(chapter):
        for child in chapter["children"]:
            attach(child)
        title = "".join(chapter["title"].split())
        targets = chapter["targets"]
        if chapter.get("presentation_role", "body") != "body" or not title.endswith("方案") or len(targets) != 1:
            return
        form, = targets
        if (form["kind"] != "form" or not form.get("manual_fill") or form.get("support_origins") != ["tender"]
                or not form.get("rule_refs") or any(form.get(field) for field in (
                    "template_refs", "response_table", "response_form_pending"))):
            return
        lines = ["".join(line.split()) for line in form["purpose"].splitlines() if line.strip()]
        if (len(lines) >= 2 and _FORMAT_ITEM.sub("", lines[0], count=1) == title
                and _PROPOSAL_INSTRUCTION.fullmatch("".join(lines[1:]))):
            targets.append(_knowledge_answer(form, form["rule_refs"]))
            form["rule_refs"] = []

    for chapter in outline["chapters"]:
        attach(chapter)


def tender_content_hashes(artifacts, snapshot):
    """Original file hashes of the frozen tender. Knowledge hits with these hashes are not evidence."""
    identity = tender_source_identity(artifacts, snapshot)
    return identity["source_shas"]


def tender_source_identity(artifacts, snapshot):
    """Document ids and original hashes. A fragment hash is not this identity."""
    frozen_ref = (snapshot or {}).get("frozen_ref")
    identity = {"document_ids": [], "source_shas": []}
    if not isinstance(frozen_ref, dict) or artifacts is None:
        return identity
    try:
        frozen = artifacts.read_json(frozen_ref)
    except (OSError, ValueError, KeyError, TypeError):
        return identity
    for document in frozen.get("documents") or []:
        document_id = document.get("document_id")
        digest = (document.get("original_ref") or {}).get("sha256")
        if isinstance(document_id, str) and document_id not in identity["document_ids"]:
            identity["document_ids"].append(document_id)
        if isinstance(digest, str) and digest not in identity["source_shas"]:
            identity["source_shas"].append(digest)
    return identity


def _as_identity(identity):
    if isinstance(identity, dict):
        return {
            "document_ids": set(identity.get("document_ids") or []),
            "source_shas": set(identity.get("source_shas") or []) | set(identity.get("content_hashes") or []),
        }
    return {"document_ids": set(), "source_shas": {item for item in identity or [] if isinstance(item, str)}}


def exclude_tender_materials(materials, identity):
    """Drop hits that are the current tender, including fragments of its original file."""
    blocked = _as_identity(identity)
    if not blocked["document_ids"] and not blocked["source_shas"]:
        return list(materials or [])
    kept = []
    for material in materials or []:
        locator = material.get("locator") or {}
        source_ids = {
            locator.get("document_id"),
            locator.get("source_document_id"),
        }
        source_shas = {
            material.get("content_sha256"),
            locator.get("source_sha256"),
            locator.get("original_sha256"),
            locator.get("document_sha256"),
        }
        original = material.get("original_ref")
        if isinstance(original, dict):
            source_shas.add(original.get("sha256"))
        if source_ids & blocked["document_ids"] or source_shas & blocked["source_shas"]:
            continue
        if blocked["source_shas"] and locator.get("source_identity") == "unconfirmed":
            continue
        kept.append(material)
    return kept


async def apply_retrieved_knowledge(knowledge, search, artifacts, identity, answer_targets):
    """Search, drop the current tender, and leave an answer blank when nothing usable remains."""
    retrieved = await knowledge.search(search, artifacts)
    found = list((retrieved or {}).get("materials") or [])
    blocked = _as_identity(identity)
    quarantined = []
    if blocked["source_shas"] or blocked["document_ids"]:
        for material in found:
            locator = material.get("locator") or {}
            if locator.get("source_identity") == "unconfirmed":
                for target_id in locator.get("target_ids") or [None]:
                    quarantined.append({"code": "knowledge_source_unconfirmed",
                        "detail": "检索命中没有原件身份，不能确认它不是本次招标文件，已隔离待核。",
                        "target_id": target_id})
    materials = exclude_tender_materials(found, identity)
    answers = []
    for target in answer_targets or []:
        if target.get("support_origins") != ["knowledge"]:
            continue
        usable = [item for item in materials if str(item.get("text") or "").strip()]
        answers.append({"target_id": target.get("target_id"), "text": "" if not usable else None, "blank": not usable})
    return {"materials": materials, "issues": [*list((retrieved or {}).get("issues") or []), *quarantined],
            "answers": answers}


def blank_knowledge_answer(tender_target, answer_target):
    """No usable knowledge leaves the answer empty and the tender wording unchanged."""
    tender_text = tender_target.get("purpose")
    answer_target["text"] = ""
    tender_target["purpose"] = tender_text
    return answer_target


def fair_select(groups, limit):
    """Round-robin whole hits. Rank is only meaningful inside one query."""
    queues = {key: list(values) for key, values in groups.items()}
    selected, seen = [], set()
    while len(selected) < limit and any(queues.values()):
        for key in list(queues):
            if len(selected) >= limit:
                break
            while queues[key] and queues[key][0] in seen:
                queues[key].pop(0)
            if not queues[key]:
                continue
            identity = queues[key].pop(0)
            seen.add(identity)
            selected.append(identity)
    omitted = [identity for values in groups.values() for identity in values if identity not in seen]
    return selected, list(dict.fromkeys(omitted))


def unit_fingerprint(unit):
    body = {key: unit.get(key) for key in ("status", "blocks", "cell_fills", "row_fills", "attachments")}
    return hashlib.sha256(_json(body).encode()).hexdigest()


def _target_positions(target):
    rows = (target.get("response_table") or {}).get("rows") or []
    row_rules = set()
    for row in rows:
        for rule in row.get("rule_refs") or [None]:
            row_rules.add(rule_ref_key(rule))
            yield rule, row
    for rule in target.get("rule_refs") or ([] if rows else [None]):
        if rule_ref_key(rule) not in row_rules:
            yield rule, None


def coverage_scope_index(tasks):
    """Index explicit positions once; never expand coverage by all same-target tasks."""
    index = {}
    for task in tasks:
        for target in task["targets"]:
            for rule, row in _target_positions(target):
                identity = coverage_identity(rule, target["target_id"], (row or {}).get("row_id"))
                index.setdefault(identity, []).append(task["task_id"])
    return {key: list(dict.fromkeys(values)) for key, values in index.items()}


def task_coverage_refs(task, payload, rule_refs):
    """Host-only link from formal positions to aliases actually present in this payload."""
    aliases = {rule_ref_key(reference): alias for alias, reference in rule_refs.items()}
    row_views = iter(payload.get("response_rows") or [])
    result = {}
    for target, view in zip(task["targets"], payload.get("targets", [])):
        projected_rows = {row["row_id"]: next(row_views, None)
                          for row in (target.get("response_table") or {}).get("rows", [])}
        for rule, row in _target_positions(target):
            row_view = projected_rows.get(row["row_id"]) if row else None
            if row is not None and row_view is None:
                continue
            rule_ids = [] if rule is None else [aliases.get(rule_ref_key(rule))]
            if rule is not None and (not rule_ids[0] or rule_ids[0] not in (row_view or view).get("rule_ids", [])):
                continue
            identity = coverage_identity(rule, target["target_id"], (row or {}).get("row_id"))
            result[identity] = {"task_id": task["task_id"], "target_id": view["target_id"], "rule_ids": rule_ids,
                                **({"row_id": row_view["row_id"]} if row_view else {})}
    return result


def _allowed_content(record, reference):
    result = []
    row = reference.get("row_id")
    target = reference["target_id"]
    rows = {value["row_id"] for value in record.get("response_rows", []) if value.get("target_id") == target}
    supports = {value["support_id"]: value for value in record.get("supports", [])}
    for identity, value in _content_ids(record).items():
        if row:
            allowed = identity.startswith("r") and value.get("row_id") == row
        elif identity.startswith("r"):
            allowed = value.get("row_id") in rows
        elif identity.startswith("a"):
            allowed = target in supports.get(value.get("attachment_id"), {}).get("target_ids", [])
        else:
            allowed = target in value.get("target_ids", [])
        if allowed:
            result.append(identity)
    return result


def batch_coverage_view(entries, tasks, batch, *, scope_index=None):
    """Use actual task aliases, with complete and provided scope kept separately."""
    scope_index = coverage_scope_index(tasks) if scope_index is None else scope_index
    records = {record["task_id"]: record for record in batch}
    items, mapping = [], {}
    for entry in entries:
        if entry.get("resolution") != "content" or not entry.get("target_id"):
            continue
        required = scope_index.get(entry["coverage_id"], [])
        references = [records[identity].get("_coverage_refs", {}).get(entry["coverage_id"])
                      for identity in required if identity in records]
        references = [reference for reference in references if reference is not None]
        if not references:
            continue
        reviewed = [reference["task_id"] for reference in references]
        complete = set(required) <= set(reviewed)
        short = f"c{len(items) + 1}"
        mapping[short] = {"coverage_id": entry["coverage_id"], "scope_complete": complete,
                          "required_task_ids": required, "reviewed_task_ids": reviewed,
                          "allowed_content_ids": {r["task_id"]: _allowed_content(records[r["task_id"]], r)
                                                  for r in references}}
        items.append({"coverage_id": short, "refs": references, "scope": "complete" if complete else "partial"})
    return items, mapping


def _content_ids(record):
    found = {}
    for key in ("blocks", "cell_fills", "row_fills", "attachments"):
        for item in (record.get("content") or {}).get(key, []):
            identity = item.get("content_id")
            if identity:
                found[identity] = item
    return found


def _has_value(identity, value, supports=None):
    """The same four content forms exist in projected review and adopted units."""
    if identity.startswith("a"):
        if supports is not None:
            support = supports.get(value.get("attachment_id"), {})
            return (support.get("origin") == "knowledge" and support.get("attachment") is True
                    and bool(str(support.get("text") or "").strip()))
        from docreader.tender.schema import ArtifactRef
        try:
            ArtifactRef.model_validate(value.get("artifact_ref"))
        except ValidationError:
            return False
        return bool(value.get("material_id"))
    return bool(str(value.get("text") or "").strip()
                or identity.startswith("b") and (
                    any(str(header).strip() for header in value.get("headers", []))
                    or any(any(str(cell).strip() for cell in row) for row in value.get("rows", []))))


def accept_assessments(raw, mapping, batch):
    """Drop bad assessment items. Keep the rest of the review packet."""
    from docreader.tender.schema import RequirementAssessmentResponse

    if not isinstance(raw, list):
        return []
    contents = {record["task_id"]: _content_ids(record) for record in batch}
    supports = {record["task_id"]: {support["support_id"]: support for support in record.get("supports", [])}
                for record in batch}
    accepted = []
    for item in raw:
        try:
            parsed = RequirementAssessmentResponse.model_validate(item).model_dump()
        except ValidationError:
            continue
        meta = mapping.get(parsed["coverage_id"])
        if meta is None:
            continue
        reviewed = meta.get("reviewed_task_ids") or list(contents)
        required = meta.get("required_task_ids") or reviewed
        refs, valid = [], True
        for ref in parsed["content_refs"]:
            present = contents.get(ref["task_id"])
            allowed = meta.get("allowed_content_ids", {}).get(ref["task_id"])
            if (ref["task_id"] not in reviewed or present is None
                    or any(identity not in present or allowed is not None and identity not in allowed
                           for identity in ref["content_ids"])):
                valid = False
                break
            if parsed["result"] in {"supported", "partial"} and any(
                    not _has_value(identity, present[identity], supports[ref["task_id"]]) for identity in ref["content_ids"]):
                valid = False
                break
            refs.append(ref)
        if not valid or (parsed["result"] in {"supported", "partial"} and not refs):
            continue
        result = parsed["result"]
        incomplete = not meta["scope_complete"]
        if result == "supported" and incomplete:
            result = "partial"
        accepted.append({
            "coverage_id": meta["coverage_id"], "result": result, "content_refs": refs,
            "scope_complete": meta["scope_complete"] and not incomplete,
            "context_incomplete": incomplete, "detail": parsed.get("detail") or "",
            "task_ids": list(reviewed), "required_task_ids": list(required), "reviewed_task_ids": list(reviewed),
        })
    return accepted


def _attach_query_gaps(entry, queries, issues):
    own = {item["query"] for item in queries if item["query_id"] in entry.get("query_ids", [])}
    for issue in issues:
        code = issue.get("code")
        if code not in _QUERY_GAP:
            continue
        query = issue.get("query")
        if query:
            if query in own and code not in entry["gap_codes"]:
                entry["gap_codes"].append(code)
        elif code == "knowledge_unconfigured" and entry.get("query_ids") and code not in entry["gap_codes"]:
            entry["gap_codes"].append(code)


def _adopted(entry, units, unit_refs, selected=None):
    found, materials = [], []
    chosen = None if selected is None else {(ref["task_id"], identity)
        for item in selected for ref in item.get("content_refs", []) for identity in ref["content_ids"]}
    for unit, ref in zip(units, unit_refs):
        if entry.get("target_id") not in (unit.get("target_ids") or []):
            continue
        if entry.get("row_id"):
            pairs = [("r", unit.get("row_fills") or [], "row_id", entry["row_id"])]
        else:
            pairs = [("b", unit.get("blocks") or [], "target_ids", entry["target_id"]),
                     ("f", unit.get("cell_fills") or [], "target_ids", entry["target_id"]),
                     ("r", unit.get("row_fills") or [], None, None),
                     ("a", unit.get("attachments") or [], None, None)]
        for prefix, values, field, expected in pairs:
            for index, value in enumerate(values, 1):
                identity = f"{prefix}{index}"
                if chosen is not None and (unit["task_id"], identity) not in chosen:
                    continue
                haystack = value.get(field)
                matched = field is None or (haystack == expected if field == "row_id" else expected in (haystack or []))
                if matched and _has_value(identity, value):
                    found.append({"unit_ref": ref, "content_id": identity})
                    materials.extend([value["material_id"]] if prefix == "a" else value.get("material_ids") or [])
    return found, list(dict.fromkeys(materials))


def _matching(entry, assessments, units_by_task):
    matched = []
    for item in assessments:
        if item.get("coverage_id") != entry["coverage_id"]:
            continue
        fingerprints = item.get("unit_fps") or {}
        tasks = item.get("reviewed_task_ids") or []
        required = item.get("required_task_ids") or []
        if not tasks or not required or set(tasks) != set(fingerprints) or item.get("review_round") not in {0, 1}:
            continue
        if item["result"] in {"supported", "partial"} and not item.get("content_refs"):
            continue
        if any(units_by_task.get(task_id) is None or unit_fingerprint(units_by_task[task_id]) != fingerprints.get(task_id)
               for task_id in tasks):
            continue
        matched.append(item)
    # A partial new batch cannot replace a complete judgment of the same adopted version.
    complete = [item for item in matched if item.get("scope_complete") and not item.get("context_incomplete")
                and set(item["required_task_ids"]) <= set(item["reviewed_task_ids"])]
    candidates = complete or matched
    if not candidates:
        return []
    latest = max(item["review_round"] for item in candidates)
    return [item for item in candidates if item["review_round"] == latest]


def _judge(matched):
    if not matched:
        return None, "unreviewed"
    results = []
    for item in matched:
        result = item["result"]
        if item.get("context_incomplete") and result == "supported":
            result = "partial"
        results.append(result)
    incomplete = any(item.get("context_incomplete") for item in matched)
    if incomplete:
        return "partial", "reviewed"
    if len(set(results)) != 1:
        return None, "unreviewed"
    return results[0], "reviewed"


def settle_coverage(entries, units, unit_refs, assessments, knowledge_issues=(), queries=()):
    units_by_task = {unit.get("task_id"): unit for unit in units}
    settled = []
    for source in entries:
        entry = {**source, "gap_codes": list(source.get("gap_codes") or []), "query_ids": list(source.get("query_ids") or [])}
        _attach_query_gaps(entry, queries, knowledge_issues)
        draft, _ = _adopted(entry, units, unit_refs)
        matched = _matching(entry, assessments, units_by_task)
        assessment, status = _judge(matched)
        content, materials = _adopted(entry, units, unit_refs, matched if status == "reviewed" else [])
        if entry["resolution"] in _SETTLED:
            status = "not_required"
            assessment = None
            content = []
        elif entry["resolution"] == "pending":
            status = "unreviewed"
            if "coverage_target_missing" not in entry["gap_codes"]:
                entry["gap_codes"].append("coverage_target_missing")
        else:
            if not draft and "coverage_unfilled" not in entry["gap_codes"]:
                entry["gap_codes"].append("coverage_unfilled")
            if status == "unreviewed" and "coverage_unassessed" not in entry["gap_codes"]:
                entry["gap_codes"].append("coverage_unassessed")
            if assessment == "partial" and "coverage_partial" not in entry["gap_codes"]:
                entry["gap_codes"].append("coverage_partial")
            if assessment == "unsupported" and "coverage_unsupported" not in entry["gap_codes"]:
                entry["gap_codes"].append("coverage_unsupported")
            if any(item.get("context_incomplete") for item in matched) and "coverage_context_incomplete" not in entry["gap_codes"]:
                entry["gap_codes"].append("coverage_context_incomplete")
            if assessment == "supported" and (not content or status != "reviewed"):
                assessment, status = None, "unreviewed"
        if entry["resolution"] == "manual" and "coverage_unfilled" not in entry["gap_codes"]:
            entry["gap_codes"].append("coverage_unfilled")
        entry.update(material_ids=materials if entry["resolution"] == "content" else [],
                     content_refs=content if entry["resolution"] == "content" else [],
                     assessment=assessment, review_status=status)
        settled.append(entry)
    return settled


def pending_outline_issues(outline):
    """Keep explicit unresolved source decisions separate from routine directory notices."""
    codes = {
        "outline_directory_applicability_unconfirmed", "outline_directory_conflict",
        "outline_directory_requirement_unassigned", "outline_directory_partial_table",
        "outline_directory_table_unbound", "outline_directory_sparse_page",
        "outline_directory_ocr_unconfirmed", "outline_directory_revision_unreviewed",
        "outline_directory_attachment_unconfirmed", "outline_directory_image_unconfirmed",
        "outline_directory_material_rejected", "outline_directory_requirement_rejected",
        "outline_directory_parameter_rejected", "outline_directory_rejected",
        "outline_directory_name_unconfirmed", "outline_directory_binding_rejected",
        "outline_directory_extra_unconfirmed",
        "outline_directory_table_header_unconfirmed",
        "outline_directory_symbol_unconfirmed",
        "outline_directory_requirement_overlap_unconfirmed",
        "outline_directory_response_form_unconfirmed", "outline_directory_response_form_unbound",
        "outline_directory_reviewed_fact_pending", "outline_directory_native_source_unavailable",
        "outline_directory_classification_rejected",
    }
    found = {}
    for issue in (outline or {}).get("issues", []):
        detail = (issue.get("detail") or "").strip()
        if issue.get("code") in codes and detail:
            found.setdefault((issue["code"], detail), issue)
    return list(found.values())


def coverage_headline(coverage, outline=None):
    if not isinstance(coverage, dict) or not isinstance(coverage.get("entries"), list):
        return {"state": "unknown", "pending": None, "unreviewed": None, "headline": "覆盖未知"}
    entries = coverage["entries"]

    def pending(entry):
        if entry.get("resolution") in {"manual", "submission", "pending"}:
            return True
        if entry.get("gap_codes"):
            return True
        if entry.get("assessment") in {"partial", "unsupported"}:
            return True
        return entry.get("resolution") == "content" and not entry.get("content_refs")

    missing = sum(1 for entry in entries if pending(entry))
    unreviewed = (sum(1 for entry in entries if entry.get("review_status") == "unreviewed")
                  + len(pending_outline_issues(outline)))
    return {"state": "draft", "pending": missing, "unreviewed": unreviewed,
            "headline": f"草稿已生成，{missing}项待补、{unreviewed}项待核"}
