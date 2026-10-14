"""Immutable obligation bindings and conservative checks of plan wording."""

import copy
import hashlib
import json
import re
import unicodedata
from decimal import Decimal, InvalidOperation

from docreader.tender.checks.verify import NUMBER
from docreader.tender.schema import issue


SNAPSHOT_FIELDS = (
    "requirement_id", "text", "subject", "evidence", "conditions", "exceptions", "numbers",
    "scoring", "veto_risk", "cross_references", "applicable", "applicability_basis",
)
# Longest matching complete unit wins: working days, calendar days and unspecified
# days must remain distinct, as must yuan/ten-thousand yuan and metre/square metre.
UNITS = (
    "个工作日", "个自然日", "个日历天", "个日历日", "工作日", "自然日", "日历天", "日历日",
    "business days", "working days", "calendar days", "business day", "working day", "calendar day",
    "days", "day", "hours", "hour", "人民币万元", "人民币元", "万元", "亿元", "美元", "元",
    "平方米", "立方米", "平方厘米", "立方厘米", "平方毫米", "千米", "厘米", "毫米", "分钟",
    "小时", "天", "日", "周", "月", "年", "分", "%", "‰", "米", "吨", "公斤", "千克",
    "个", "件", "套", "份", "人", "项", "页", "kg", "cm", "mm", "m²", "m³", "m2", "m3", "m",
)
COMPARATORS = {
    "gte": ("不少于", "不低于", "不得少于", "不得低于", "至少", "最低", "≥", ">=", "at least", "no less than", "minimum"),
    "lte": ("不超过", "不多于", "不高于", "不得超过", "不得多于", "不得高于", "至多", "最高", "≤", "<=", "at most", "no more than", "up to", "within", "maximum"),
    "gt": ("大于", "高于", "多于", "超过", ">", "more than", "greater than", "over"),
    "lt": ("小于", "低于", "少于", "不足", "<", "less than", "under", "below"),
    "eq": ("等于", "恰好", "正好", "=", "equal to", "exactly"),
}


def _compact(text: str) -> str:
    return re.sub(r"\s+", "", unicodedata.normalize("NFKC", text)).casefold()


_COMPARATOR_VALUES = {_compact(token): name for name, tokens in COMPARATORS.items() for token in tokens}


def _token_pattern(token: str) -> str:
    token = unicodedata.normalize("NFKC", token).casefold()
    pattern = r"\s+".join(re.escape(word) for word in token.split())
    return rf"(?<![a-z]){pattern}(?![a-z])" if token.isascii() and any(char.isalpha() for char in token) else pattern


_COMPARATOR = re.compile("|".join(_token_pattern(token) for token in sorted(
    {token for tokens in COMPARATORS.values() for token in tokens}, key=len, reverse=True)))


def _value(value) -> str:
    value = _compact(str(value)).replace(",", "")
    try:
        parsed = Decimal(value)
        return format(parsed.normalize(), "f") if parsed.is_finite() else value
    except InvalidOperation:
        return value


def _relations(text: str, start: int, end: int) -> list[str]:
    left = re.split(r"[，,。；;\n]", text[:start])[-1]
    # A comparison before a different quantity does not silently carry forward.
    previous_numbers = list(NUMBER.finditer(left))
    if previous_numbers:
        left = left[previous_numbers[-1].end():]
    found = list(_COMPARATOR.finditer(left))
    relations = {_COMPARATOR_VALUES[_compact(found[-1].group())]} if found else set()
    suffix = text[end:].lstrip()
    if re.match(r"^(?:及)?以上", suffix):
        relations.add("gte")
    if re.match(r"^(?:及)?以下|^以内|^内", suffix):
        relations.add("lte")
    return sorted(relations)


def _quantities(text: str, numbers: list[dict]) -> list[dict]:
    text = unicodedata.normalize("NFKC", text).casefold()
    units = {unicodedata.normalize("NFKC", number["unit"]).casefold() for number in numbers if number["unit"]}
    units.update(unicodedata.normalize("NFKC", unit).casefold() for unit in UNITS)
    unit_pattern = re.compile(r"\s*(?:" + "|".join(_token_pattern(unit) for unit in sorted(units, key=len, reverse=True)) + ")")
    mentions = []
    for match in NUMBER.finditer(text):
        unit = unit_pattern.match(text, match.end())
        unit_value, end = (_compact(unit.group()), unit.end()) if unit else ("", match.end())
        mentions.append({"value": _value(match.group()), "unit": unit_value,
                         "comparators": _relations(text, match.start(), end)})
    for number in numbers:
        literal = _compact(str(number["value"]))
        try:
            Decimal(literal.replace(",", ""))
            continue
        except InvalidOperation:
            pass
        for match in re.finditer(re.escape(literal) + r"\s*" + _token_pattern(number["unit"]), text):
            mentions.append({"value": _value(literal), "unit": _compact(number["unit"]),
                             "comparators": _relations(text, match.start(), match.end())})
    return mentions


def _obligation_quantities(requirement: dict) -> list[dict]:
    numbers = requirement["numbers"]
    source_mentions = _quantities("；".join([requirement["text"], *requirement["conditions"], *requirement["exceptions"]]), numbers)
    obligations = []
    for number in numbers:
        expected = {"value": _value(number["value"]), "unit": _compact(number["unit"])}
        occurrences = [mention for mention in source_mentions if all(mention[key] == value for key, value in expected.items())]
        if not occurrences:
            occurrences = [mention for mention in _quantities(number["quote"], numbers)
                           if all(mention[key] == value for key, value in expected.items())]
        for occurrence in occurrences or [{**expected, "comparators": []}]:
            if occurrence not in obligations:
                obligations.append(occurrence)
    return obligations


def _canonical(value: dict) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False)


def _digest(snapshot: dict) -> str:
    return hashlib.sha256(_canonical(snapshot).encode("utf-8")).hexdigest()


def constraint_binding(requirement: dict) -> dict:
    """Compute a detached validation snapshot; only its digest is transmitted/saved."""
    snapshot = {"schema_version": 1, **{field: copy.deepcopy(requirement[field]) for field in SNAPSHOT_FIELDS},
                "comparators": _obligation_quantities(requirement)}
    return {"constraint_snapshot": snapshot, "constraint_digest": _digest(snapshot)}


def _literal_preserved(literal: str, text: str) -> bool:
    literal, text = _compact(literal), _compact(text)
    for match in re.finditer(re.escape(literal), text):
        prefix, suffix = text[:match.start()], text[match.end():]
        if re.search(r"(?:不|非|不得|无需|不能|并非|而非|不是|not|except)(?:适用(?:于)?|包括|属于|针对|for|to)?$", prefix):
            continue
        if re.match(r"^(?:不适用|无效|notapplicable|excluded)", suffix):
            continue
        return True
    return False


def check_plan_constraints(item: dict, requirement: dict, binding: dict) -> list[dict]:
    findings = []
    requirement_id = requirement["requirement_id"]
    try:
        digest_valid = item["constraint_digest"] == binding["constraint_digest"] == _digest(binding["constraint_snapshot"])
    except (TypeError, ValueError):
        digest_valid = False
    if not digest_valid:
        findings.append(issue("plan_constraint_binding_mismatch", "Plan digest does not exactly bind the verified obligation.", requirement_id=requirement_id))
    # Binding correctness is necessary but cannot excuse contradictory plan prose.
    text = item["target"] + "；" + item["action"]
    for field, code in (("subject", "plan_subject_changed"), ("conditions", "plan_condition_changed"), ("exceptions", "plan_exception_changed")):
        values = [requirement[field]] if isinstance(requirement[field], str) else requirement[field]
        if any(not value.strip() or not _literal_preserved(value, item["action"]) for value in values):
            findings.append(issue(code, f"Plan wording must preserve the obligation's {field} literally, without negation.", requirement_id=requirement_id))
    expected = binding["constraint_snapshot"]["comparators"]
    mentions = _quantities(text, requirement["numbers"])
    expected_values = {entry["value"] for entry in expected}
    expected_pairs = {(entry["value"], entry["unit"]) for entry in expected}
    for obligation in expected:
        matches = [entry for entry in mentions if (entry["value"], entry["unit"]) == (obligation["value"], obligation["unit"])]
        if not matches:
            findings.append(issue("plan_numeric_obligation_missing", "The response omits a required value with its complete original unit.", requirement_id=requirement_id))
        elif not any(entry["comparators"] == obligation["comparators"] for entry in matches):
            findings.append(issue("plan_comparator_changed", "Plan changes or omits an obligation's lower/upper bound or comparison.", requirement_id=requirement_id))
    # Catch a correct binding/target accompanied by a contradictory action using
    # the same bare number but another unit or comparison. Ambiguity stays blocked.
    for mention in _quantities(item["action"], requirement["numbers"]):
        if mention["value"] not in expected_values:
            continue
        if (mention["value"], mention["unit"]) not in expected_pairs:
            findings.append(issue("plan_unit_changed", "Plan action changes the unit or day convention attached to an obligation value.", requirement_id=requirement_id))
        elif mention not in expected:
            findings.append(issue("plan_comparator_changed", "Plan action introduces a different comparison for an obligation value.", requirement_id=requirement_id))
    return findings
