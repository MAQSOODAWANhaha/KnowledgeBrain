"""Preserve frozen requirements without depending on generated bidder answers."""

from copy import deepcopy
import json

from pydantic import ValidationError

from docreader.tender.schema import AuthoringRequirements, EffectiveItemRef, EffectiveRule, TenderItem


def preserve_requirements(context, outline) -> dict:
    """Project known rule identities and current semantics; never infer from sources."""
    rows = {}
    issues = [{"code": entry["code"], "detail": entry["detail"]}
              for entry in context.availability_issues]
    views = {entry["rule_id"]: entry for entry in context.payload.get("rules", [])}

    def add(reference, view=None):
        if isinstance(reference, dict):
            reference = {key: value for key, value in reference.items()
                         if key not in {"decision_id", "rule_index"} or value is not None}
        try:
            reference = EffectiveItemRef.model_validate(reference).model_dump(exclude_unset=True)
        except ValidationError:
            issues.append({"code": "authoring_requirement_reference_unavailable",
                "detail": "部分要求的正式引用不符合当前合同，原分析及大纲引用仍保留，相关位置待核。"})
            return None
        identity = json.dumps(reference, sort_keys=True, separators=(",", ":"))
        row = rows.setdefault(identity, {"rule_ref": reference, "rule": None, "target_ids": []})
        if view is not None and row["rule"] is None:
            field, contract = ("effective_rule", EffectiveRule) if "effective_rule" in view else ("item", TenderItem)
            raw = view.get(field)
            if isinstance(raw, dict):
                row["rule"] = deepcopy(raw)
                try:
                    contract.model_validate(deepcopy(raw))
                except ValidationError:
                    issues.append({"code": "authoring_requirement_contract_unverified",
                        "detail": "要求“" + reference["item_ref"]["item_id"]
                            + "”含当前合同未确认的字段，原记录完整保留，相关内容待核。"})
        return row

    for identity, reference in context.rules.items():
        add(reference, views.get(identity))

    def bind(chapters):
        for chapter in chapters:
            for target in chapter.get("targets", []):
                for reference in target.get("rule_refs", []):
                    row = add(reference)
                    if row is not None and target["target_id"] not in row["target_ids"]:
                        row["target_ids"].append(target["target_id"])
            bind(chapter.get("children", []))

    bind(outline.get("chapters", []))
    for row in rows.values():
        if row["rule"] is None:
            issues.append({"code": "authoring_requirement_unavailable",
                "detail": "要求“" + row["rule_ref"]["item_ref"]["item_id"]
                    + "”的当前语义不可读取或不符合字段合同，保留正式引用和已关联位置，继续编制。"})
    if set(views) - context.rules.keys():
        issues.append({"code": "authoring_requirement_reference_unavailable",
            "detail": "部分已有要求未能绑定正式引用，原分析记录保留，相关要求待核对。"})
    origin = context.rule_origin
    if origin == "source_only":
        issues.append({"code": "authoring_requirements_source_only",
            "detail": "当前只有来源，尚无可保留的事项台账；不将背景、评审表或整段原文推断为响应要求。"})
    issues = list({json.dumps(entry, sort_keys=True): entry for entry in issues}.values())
    return AuthoringRequirements(schema_version=2, authoring_input=deepcopy(context.input),
        rule_origin=origin, rules=list(rows.values()), issues=issues).model_dump(exclude_unset=True)
