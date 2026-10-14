"""Continuous source excerpts and conservative cumulative section conditions."""
from __future__ import annotations

import hashlib
import re
from collections import defaultdict

_CONDITION = re.compile(
    r"适用|除非|仅(?:限|第|对|本|在)|如果|如属|如为|联合体|条件下|"
    r"\bif\b|\bonly\b|except|unless|\bappl(?:y|ies|ying|icab\w*)\b|provided that|consorti|joint venture",
    re.I,
)
_EXPLICIT_SECTION = re.compile(
    r"本(?:章|节|部分)(?:全部|所有)?(?:要求|条款|规定)?(?:均|仅|只|适用|不适用)|"
    r"以下(?:条款|要求)|下列(?:条款|要求)|"
    r"(?:this|following)\s+(?:section|chapter|requirements|provisions)\s+(?:appl|only)", re.I,
)
_REPLACEMENT = re.compile(r"取代|替代|改为|变更为|replace|supersede|instead|notwithstanding", re.I)
_TERMINATOR = re.compile(r"[。！？；;!?][\"'”’）)]*|\.(?=\s+[A-Z]|\s*$)[\"'”’)]*|\n\s*\n")
_SCOPE_ONLY = re.compile(
    r"^(?:仅|只)(?:限于?|对|有)?[^。；;\n]*适用|"
    r"^(?:适用范围|applicability|scope)\s*[:：]|"
    r"^(?:only|(?:applicable|apply|applies|applying)\s+only\s+to)\s+.*(?:appl|lot|section)", re.I,
)
_TOPICAL_CONDITION = re.compile(
    r"^(?:联合体|consorti\w*|joint\s+ventures?)\s*(?:投标人|投标|成员)?\s*(?:还需|还须|须|必须|应|shall|must)", re.I,
)
_NAMED_SCOPE = re.compile(
    r"本(?:表|格式|附件|项目|工程)|(?:该|此)(?:表|格式|附件)|"
    r"(?:附件|附录|格式)\s*[一二三四五六七八九十\d]|"
    r"(?:this|following)\s+(?:form|appendix|project)|"
    r"(?:适用|适合)于.*(?:表|格式)|(?:表|格式).*(?:适用|使用)", re.I,
)
_SCOPE_BACKREF = re.compile(r"(?:上述|前述|以上|前款).*(?:范围|适用)|(?:previous|above)\s+scope", re.I)
_SCOPE_REFERENCE = re.compile(r"(?:详?见|参见|按照|依据).*(?:前附表|适用范围)|(?:see|refer to).*applicability", re.I)
_LIST_CONDITION = re.compile(r"^(?:如(?:果|属|为)?|若|当|凡|仅当|if\b|unless\b).*[：:]\s*$", re.I | re.S)
_PRIORITY = re.compile(r"(?:解释|效力|适用).{0,12}(?:顺序|优先)|(?:先后)?顺序解释|order\s+of\s+precedence", re.I)
_APPLICABILITY = re.compile(r"适用|是否接受|(?:接受|允许).*联合体|联合体.*(?:接受|允许)|\bappl(?:y|ies|ying|icab\w*)\b|consorti|joint venture", re.I)
_FORM_SCOPE_LINE = re.compile(r"(?:^|\n)\s*[（(]\s*适用于[^）)]+[）)]")
_LOCAL_FIELD = re.compile(r"(?:空格|空白|字段|单元格).*(?:标示|标注|填写)|不适用或.*(?:没有|无).*填写", re.S)
_DIRECT_NAMED_SCOPE = re.compile(r"本(?:表|格式|附件|项目|工程)(?:仅|只|不)?(?:适用|接受|允许)|this\s+(?:form|project)\s+(?:applies|allows)", re.I)


def _navigation(entry: dict) -> bool:
    text = entry["text"]
    title = entry.get("heading_path", "").rsplit(" > ", 1)[-1].strip()
    if re.fullmatch(r"目\s*录|contents|table\s+of\s+contents", title, re.I):
        return True
    if re.search(r"(?:^|\n)\s*(?:目\s*录|contents|table\s+of\s+contents)\s*(?:\n|$)", text, re.I) and len(
        re.findall(r"(?:^|\n)\s*(?:附件|附录|第|appendix|chapter)\s*[一二三四五六七八九十\d]", text, re.I)
    ) >= 2:
        return True
    # A contents page is identified by its navigation structure, not a keyword
    # inside one listed chapter. A single dotted form blank is not a contents page.
    return len(re.findall(r"(?:\.{3,}|…{2,}|·{3,})\s*\d+\s*(?:\n|$)", text)) >= 2


def _scope_role(text: str, preamble: bool, has_scope: bool) -> str | None:
    """Only cross-range declarations become context; local duties stay ordinary body."""
    text = text.strip()
    if _CONDITION.search(text) and _EXPLICIT_SECTION.search(text):
        return "explicit_section"
    if _DIRECT_NAMED_SCOPE.search(text) and _APPLICABILITY.search(text):
        return "target_requires_binding"
    if _LOCAL_FIELD.search(text):
        return None
    if _SCOPE_BACKREF.search(text) and _REPLACEMENT.search(text):
        return "scope_modification"
    if (_APPLICABILITY.search(text) and (_NAMED_SCOPE.search(text) or _SCOPE_REFERENCE.search(text))) or _FORM_SCOPE_LINE.search(text):
        return "target_requires_binding"
    if _LIST_CONDITION.match(text) or _PRIORITY.search(text):
        return "target_requires_binding"
    if _SCOPE_ONLY.search(text):
        return "section_preamble" if preamble else "target_requires_binding"
    if preamble and has_scope and _TOPICAL_CONDITION.search(text):
        return "section_preamble"
    return None


def _segments(entry: dict):
    """Sentence/paragraph spans, retaining wrapped lines and source offsets."""
    text = entry["text"]
    offset = 0
    first_line, separator, _ = text.partition("\n")
    title = entry.get("heading_path", "").rsplit(" > ", 1)[-1]
    if title and first_line.lstrip("# ").strip() == title.strip():
        end = len(first_line)
        yield 0, end, True, True
        offset = end + len(separator)
    # Only native paragraph offsets may end a paragraph without punctuation.
    # PDF visual line wrapping is not such evidence. In particular, a later
    # paragraph's full stop cannot make an incomplete scope declaration complete.
    boundaries = sorted({len(text), *(boundary for boundary in entry.get("paragraph_breaks", [])
                                      if isinstance(boundary, int) and offset < boundary <= len(text))})
    for boundary in boundaries:
        for match in _TERMINATOR.finditer(text, offset, boundary):
            end = match.end()
            if text[offset:end].strip():
                yield offset, end, False, True
            offset = end
        if text[offset:boundary].strip():
            yield offset, boundary, False, False
        offset = boundary


def _excerpt(entry: dict, start: int, end: int, reason: str) -> dict:
    text = entry["text"]
    # Whitespace trimming is offset-preserving; no ellipsis or disjoint concatenation.
    start += len(text[start:end]) - len(text[start:end].lstrip())
    end -= len(text[start:end]) - len(text[start:end].rstrip())
    identity = f"{entry['range_id']}\0{start}\0{end}"
    result = {key: value for key, value in entry.items() if key not in {"cells", "image_ref"}}
    result.update(
        range_id="context-" + hashlib.sha256(identity.encode()).hexdigest()[:24],
        parent_range_id=entry["range_id"], text=text[start:end],
        start=entry["start"] + start, end=entry["start"] + end, reason=reason,
    )
    return result


def condition_context(ranges: list[dict], scope_decisions: list[dict] | tuple = ()) -> tuple[dict, dict, dict, dict]:
    """Index all preceding conditions, never silently assume a later one replaces one."""
    introductions, conditions, issues = {}, defaultdict(list), defaultdict(list)
    sections = {}
    body_started = defaultdict(bool)
    bound_scope = set()
    reviewed = {decision["range_id"]: decision for decision in scope_decisions}
    for position, entry in enumerate(ranges):
        section_id = entry.get("section_id")
        key = section_id or "unlocated"
        path = entry.get("heading_path", "")
        sections.setdefault(key, {"path": path, "parent": entry.get("parent_section_id")})
        if _navigation(entry):
            body_started[key] = True
            continue
        for start, end, heading, complete in _segments(entry):
            text = entry["text"][start:end]
            if heading:
                introductions.setdefault(key, _excerpt(entry, start, end, "introduction"))
                continue
            if not text.strip():
                continue
            preamble = entry["kind"] == "section" and not body_started[key]
            role = _scope_role(text, preamble, key in bound_scope)
            if role is None:
                if entry["kind"] == "section":
                    introductions.setdefault(key, _excerpt(entry, start, end, "introduction"))
                body_started[key] = True
                continue
            fragment = _excerpt(entry, start, end, "condition")
            decision = reviewed.get(fragment["range_id"])
            if decision and decision["decision"] == "local_only":
                # The source remains ordinary body evidence. A human decision is
                # an applicability interpretation, never a deletion of source text.
                body_started[key] = True
                continue
            explicit = role == "explicit_section"
            boundaries = entry.get("text_boundaries", {})
            complete = complete and (start > 0 or boundaries.get("start", True))
            resolved = bool(section_id and path and complete and role in {"explicit_section", "section_preamble"}
                            and not _REPLACEMENT.search(text))
            if decision:
                resolved = True
            if resolved and not decision:
                bound_scope.add(key)
            # Local table/row conditions have no demonstrated chapter-wide scope.
            table_ids = entry.get("table_ids") or ([entry["table_id"]] if entry.get("table_id") else [])
            fragment["scope_binding"] = {
                "section_id": section_id, "heading_path": path,
                "kind": "explicit_section" if resolved and explicit else "section_preamble" if resolved else "unresolved",
                "relationship": "cumulative", "status": "bound" if resolved else "unresolved",
                "table_ids": [] if explicit else table_ids,
                "complete_sentence": complete,
            }
            if decision:
                fragment["scope_binding"].update(
                    kind="reviewed_targets", reviewed_target_range_ids=decision["target_range_ids"],
                    decision_range_id=decision["range_id"],
                )
            conditions[key].append((position, fragment))
            if not resolved:
                issues[entry["range_id"]].append({
                    "code": "condition_scope_unresolved", "blocking": True,
                    "document_id": entry["document_id"], "range_id": fragment["range_id"],
                    "parent_range_id": entry["range_id"],
                    "scope_reason": role,
                    "detail": "Explicit cross-range condition needs reliable target/boundary binding; ordinary local clauses remain body evidence.",
                })
    return introductions, conditions, issues, sections


def pending_scope_metadata(fragment: dict) -> dict:
    """Keep a reviewable source reference, never a misleading long text excerpt."""
    fields = (
        "range_id", "source_range_id", "parent_range_id", "document_id", "document_version", "unit_id", "start", "end",
        "locator", "physical_locator", "physical_path", "section_id", "parent_section_id", "heading_path", "scope_binding",
    )
    result = {**{key: fragment[key] for key in fields if key in fragment},
              "reason": "incomplete_condition_sentence"}
    for key in ("locator", "physical_locator"):
        if isinstance(result.get(key), dict):
            result[key] = {name: value for name, value in result[key].items() if name not in {"cells", "cell_refs", "rendered_spans"}}
    return result


def section_ancestors(entry: dict, sections: dict) -> list[str]:
    key = entry.get("section_id") or "unlocated"
    result, seen = [], set()
    while key and key not in seen:
        result.append(key)
        seen.add(key)
        key = sections.get(key, {}).get("parent")
    return result
