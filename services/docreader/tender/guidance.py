"""Bind catalog documents to one chapter before that chapter is written.

A filename is only a label. It never filters, ranks, or adopts a document.
Vector scores may order the library, but a low score does not remove a file
and a high score does not select it. Spoken filenames are ignored.
"""

from __future__ import annotations

import json


def chapter_guide(chapter, catalog, bindings, hints=(), scores=None):
    grouped = {}
    for target in chapter.get("targets", []):
        kind = slot_class(target)
        if kind is None:
            continue
        key = _slot_key(kind, target)
        grouped.setdefault(key, {"key": key, "class": kind, "targets": []})["targets"].append(target)
    known = {document["document_id"] for document in catalog}
    hinted = [hint["document_id"] for hint in hints if hint.get("document_id") in known]
    slots = []
    for slot in grouped.values():
        candidates = _ordered(_candidates(slot["class"], catalog), scores)
        allowed = {document["document_id"] for document in candidates}
        record = bindings.get(slot["key"])
        visible = bool(record) and (
            record.get("scope") == "project" or record.get("chapter_id") == chapter.get("chapter_id"))
        slots.append({
            **slot,
            "candidates": candidates,
            "hints": [document_id for document_id in hinted if document_id in allowed],
            "content_document_ids": _content_ids(slot["targets"], candidates),
            "selected_document_ids": [document_id for document_id in record.get("document_ids", [])
                                      if document_id in allowed] if visible else [],
            "inherited": bool(visible and record.get("scope") == "project"),
        })
    return slots


def slot_class(target):
    if target.get("manual_fill"):
        return None
    origins = target.get("support_origins") or ["knowledge"]
    if origins == ["tender"] and target.get("kind") != "attachment":
        return None
    purpose = target.get("purpose") or ""
    found = []
    if "业绩" in purpose:
        found.append("performance")
    if "服务" in purpose:
        found.append("service")
    if any(word in purpose for word in ("技术", "参数", "产品", "规格")):
        found.append("product")
    if any(word in purpose for word in ("资质", "证书", "认证", "执照")):
        found.append("proof")
    found = list(dict.fromkeys(found))
    if len(found) > 1:
        return "unresolved"
    if len(found) == 1:
        return found[0]
    if target.get("kind") == "attachment":
        return "proof"
    if "knowledge" in origins:
        return "product"
    return None


def bind_slot(bindings, slot, document_ids, catalog, *, scope, chapter_id):
    if scope not in {"project", "chapter"} or not document_ids:
        return dict(bindings)
    allowed = {document["document_id"] for document in slot.get("candidates", [])}
    if set(document_ids) - allowed:
        return dict(bindings)
    return {**bindings, slot["key"]: {
        "document_ids": list(document_ids), "scope": scope, "chapter_id": chapter_id}}


def resolve_utterance(text, slots):
    return None


def tasks_for_chapters(tasks, chapter_ids):
    wanted = set(chapter_ids)
    return [task for task in tasks if task.get("chapter_id") in wanted]


def allowed_material_ids(task, slots, bindings, materials):
    selected = []
    for slot in slots:
        record = bindings.get(slot["key"])
        if not record:
            continue
        if record.get("scope") != "project" and record.get("chapter_id") != task.get("chapter_id"):
            continue
        for document_id in record.get("document_ids", []):
            if document_id not in selected:
                selected.append(document_id)
    wanted = set(selected)
    return [item["material_id"] for item in materials if item.get("document_id") in wanted]


def _slot_key(kind, target):
    refs = target.get("rule_refs") or []
    if len(refs) == 1:
        token = str(refs[0].get("item_ref", {}).get("item_id") or "")
    elif refs:
        token = json.dumps(refs, ensure_ascii=False, sort_keys=True)
    else:
        token = ""
    token = token or target["target_id"]
    subject = target.get("subject") or ""
    return f"{kind}:{subject}:{token}" if subject else f"{kind}:{token}"


def _candidates(kind, catalog):
    if kind in {"unresolved", None}:
        return list(catalog)
    return [document for document in catalog if (document.get("kind") == "product") == (kind == "product")]


def _ordered(candidates, scores):
    if not scores:
        return candidates
    return sorted(candidates, key=lambda document: (-float(scores.get(document["document_id"]) or 0),))


def _content_ids(targets, candidates):
    markers = list(dict.fromkeys(marker for target in targets
                                 for marker in _body_markers(target.get("purpose") or "")))
    if not markers:
        return []
    return [document["document_id"] for document in candidates
            if any(marker in (document.get("text") or "") for marker in markers)]


def _body_markers(purpose):
    found = []
    if "执照" in purpose:
        found.extend(("统一社会信用代码", "营业执照"))
    if "身份证" in purpose:
        found.append("公民身份号码")
    if "业绩" in purpose:
        found.extend(("甲方", "合同金额"))
    if "服务" in purpose:
        found.extend(("响应时间", "售后"))
    if any(word in purpose for word in ("证书", "认证", "资质")) and "执照" not in purpose:
        found.append("认证证书")
    return found
