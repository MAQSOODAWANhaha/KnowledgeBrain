"""Read-only projection and metric aggregation for the authoring human pending queue."""

from __future__ import annotations

from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
import math
from typing import Any

from docreader.tender.status import AuthoringGapStatus


def gap_identity(gap: dict[str, Any], occurrence: int = 0) -> str:
    """Derive a deterministic identity for one authoring gap without mutating artifacts."""
    payload = [
        gap.get("chapter_id") or "",
        gap.get("target_id") or "",
        gap.get("code") or "",
        gap.get("detail") or "",
        occurrence,
    ]
    encoded = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()[:24]


def parse_timestamp(value: Any) -> datetime | None:
    if isinstance(value, datetime):
        return value.replace(tzinfo=timezone.utc) if value.tzinfo is None else value.astimezone(timezone.utc)
    if isinstance(value, str) and value.strip():
        text = value.strip()
        if text.endswith("Z"):
            text = text[:-1] + "+00:00"
        try:
            parsed = datetime.fromisoformat(text)
        except ValueError:
            return None
        return parsed.replace(tzinfo=timezone.utc) if parsed.tzinfo is None else parsed.astimezone(timezone.utc)
    return None


def format_timestamp(value: Any) -> str | None:
    parsed = parse_timestamp(value)
    if parsed is None:
        return None
    return parsed.isoformat().replace("+00:00", "Z")


def handling_duration_ms(created_at: Any, handled_at: Any) -> int | None:
    start = parse_timestamp(created_at)
    end = parse_timestamp(handled_at)
    if start is None or end is None:
        return None
    return max(0, round((end - start).total_seconds() * 1000))


def _outline_locations(outline: dict[str, Any] | None) -> tuple[dict[str, tuple[int, str, str | None]], dict[str, tuple[str, int, int, str, str, str | None]]]:
    chapters_by_id: dict[str, tuple[int, str, str | None]] = {}
    targets_by_id: dict[str, tuple[str, int, int, str, str, str | None]] = {}
    chapter_counter = 0
    target_counter = 0

    def walk(nodes: list[dict[str, Any]]) -> None:
        nonlocal chapter_counter, target_counter
        for chapter in nodes:
            chapter_id = chapter.get("chapter_id") or ""
            chapter_title = chapter.get("title") or ""
            chapter_bookmark = chapter.get("bookmark")
            order = chapter_counter
            chapter_counter += 1
            if chapter_id and chapter_id not in chapters_by_id:
                chapters_by_id[chapter_id] = (order, chapter_title, chapter_bookmark)
            for target in chapter.get("targets") or []:
                target_id = target.get("target_id") or ""
                t_order = target_counter
                target_counter += 1
                if target_id and target_id not in targets_by_id:
                    targets_by_id[target_id] = (
                        chapter_id,
                        order,
                        t_order,
                        chapter_title,
                        target.get("purpose") or "",
                        target.get("bookmark") or chapter_bookmark,
                    )
            walk(chapter.get("children") or [])

    walk((outline or {}).get("chapters") or [])
    return chapters_by_id, targets_by_id


def build_pending_queue(
    outline: dict[str, Any] | None,
    gaps: list[dict[str, Any]] | tuple[dict[str, Any], ...],
    dispositions: list[dict[str, Any]] | tuple[dict[str, Any], ...] = (),
    *,
    created_at: Any = None,
) -> list[dict[str, Any]]:
    chapters_by_id, targets_by_id = _outline_locations(outline)
    by_gap_id: dict[str, dict[str, Any]] = {}
    for row in dispositions:
        status = AuthoringGapStatus(row["status"])
        by_gap_id[row["gap_id"]] = {**row, "status": status}

    created_iso = format_timestamp(created_at)
    seen: Counter[tuple[str, str, str, str]] = Counter()
    indexed_items: list[tuple[tuple[int, int, int], dict[str, Any]]] = []
    unmapped = 10**9

    for index, gap in enumerate(gaps or ()):
        chapter_id = gap.get("chapter_id")
        target_id = gap.get("target_id")
        code = gap.get("code") or ""
        detail = gap.get("detail") or ""
        key = (chapter_id or "", target_id or "", code, detail)
        occurrence = seen[key]
        seen[key] += 1
        gap_id = gap_identity(gap, occurrence)

        target_loc = targets_by_id.get(target_id) if target_id else None
        chapter_loc = chapters_by_id.get(chapter_id) if chapter_id else None
        if target_loc is not None:
            resolved_chapter_id, c_order, t_order, chapter_title, target_purpose, bookmark = target_loc
            effective_chapter_id = chapter_id or resolved_chapter_id
            sort_key = (c_order, t_order, index)
        elif chapter_loc is not None:
            c_order, chapter_title, bookmark = chapter_loc
            effective_chapter_id = chapter_id
            target_purpose = None
            sort_key = (c_order, unmapped, index)
        else:
            effective_chapter_id = chapter_id
            chapter_title = None
            target_purpose = None
            bookmark = None
            sort_key = (unmapped, unmapped, index)

        disposition = by_gap_id.get(gap_id)
        status = disposition["status"] if disposition else AuthoringGapStatus.PENDING
        note = (disposition.get("note") or "") if disposition else ""
        handled_raw = (disposition.get("handled_at") or disposition.get("first_handled_at")) if disposition else None
        handled_iso = format_timestamp(handled_raw) if status != AuthoringGapStatus.PENDING else None
        duration = handling_duration_ms(created_iso, handled_iso) if status != AuthoringGapStatus.PENDING else None

        indexed_items.append((sort_key, {
            "gap_id": gap_id,
            "code": code,
            "detail": detail,
            "chapter_id": effective_chapter_id,
            "chapter_title": chapter_title,
            "target_id": target_id,
            "target_purpose": target_purpose,
            "bookmark": bookmark,
            "status": status.value,
            "note": note,
            "created_at": created_iso,
            "handled_at": handled_iso,
            "handling_duration_ms": duration,
        }))

    indexed_items.sort(key=lambda pair: pair[0])
    return [item for _, item in indexed_items]


def pending_queue_summary(
    gaps: list[dict[str, Any]] | tuple[dict[str, Any], ...],
    dispositions: list[dict[str, Any]] | tuple[dict[str, Any], ...] = (),
    *,
    created_at: Any = None,
) -> dict[str, Any]:
    items = build_pending_queue(None, gaps, dispositions, created_at=created_at)
    counts = Counter(item["status"] for item in items)
    by_status = {status.value: counts.get(status.value, 0) for status in AuthoringGapStatus}
    pending = by_status[AuthoringGapStatus.PENDING.value]
    total = len(items)
    durations = sorted(
        item["handling_duration_ms"]
        for item in items
        if item["status"] != AuthoringGapStatus.PENDING.value and item["handling_duration_ms"] is not None
    )
    observations = len(durations)
    return {
        "total": total,
        "pending": pending,
        "handled": total - pending,
        "by_status": by_status,
        "handling_duration_ms": {
            "observations": observations,
            "avg": round(sum(durations) / observations) if observations else None,
            "max": durations[-1] if observations else None,
            "p95": durations[max(0, math.ceil(0.95 * observations) - 1)] if observations else None,
        },
    }
