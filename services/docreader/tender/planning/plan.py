"""A response plan must give each applicable obligation a concrete destination."""

import re

from pydantic import ValidationError

from docreader.llm import ModelSemanticError
from docreader.tender.planning.constraints import check_plan_constraints, constraint_binding
from docreader.tender.schema import Model, Plan, issue

PLACEHOLDER = re.compile(r"^(?:tbd|todo|n/?a|none|待定|待补充|待填写|无|标题|章节|附件|表格|正文|人工处理|[.。…_\- ]+)$", re.I)
GENERIC_ACTION = re.compile(r"^(?:响应|满足|遵守|处理|填写|写作|编写|生成|提供|参见|见|respond|comply|write|provide)(?:本?要求|内容|正文|附件|材料|章节|the requirement)?[。. ]*$", re.I)
LOCATION = re.compile(r"[/＞>:：#§]|第[^\s]{1,12}[章节条]|\b(?:section|chapter|appendix)\b|\d+\.\d+", re.I)
FIELD = re.compile(r"字段|单元格|列|行|\b(?:field|cell|column|row)\b|\b[A-Z]{1,3}[1-9][0-9]*\b", re.I)
ACTION = re.compile(r"编写|撰写|描述|阐述|说明|承诺|列明|列出|填写|填入|提交|附上|提供|证明|计算|核对|审核|确认|签署|盖章|办理|准备|展示|引用|声明|响应|制定|安排|检查|标注|上传|获取|对照|填写|\b(?:state|describe|explain|commit|list|fill|enter|submit|attach|provide|demonstrate|calculate|check|verify|review|confirm|sign|prepare|show|cite|declare|write|schedule|upload|obtain|include|document)\b", re.I)


def _nontrivial(text: str) -> bool:
    return bool(text.strip()) and not PLACEHOLDER.fullmatch(text.strip())


def _item_issues(item: dict, requirement: dict, binding: dict) -> list[dict]:
    issues = []
    requirement_id = item["requirement_id"]
    kind, target, action = item["target_kind"], item["target"].strip(), item["action"].strip()
    if not _nontrivial(target):
        issues.append(issue("plan_target_placeholder", "Plan target must identify an actual response position.", requirement_id=requirement_id))
    if kind == "body" and not LOCATION.search(target):
        issues.append(issue("plan_body_location_missing", "Body target must identify a chapter/section and position.", requirement_id=requirement_id))
    if kind == "table_field" and (not FIELD.search(target) or not LOCATION.search(target)):
        issues.append(issue("plan_table_field_missing", "Table target must identify the specific table and field/cell.", requirement_id=requirement_id))
    if not _nontrivial(action) or GENERIC_ACTION.fullmatch(action) or action == target or not ACTION.search(action):
        issues.append(issue("plan_action_placeholder", "Plan action must explain how this requirement will be answered.", requirement_id=requirement_id))
    if not all(_nontrivial(need) for need in item["material_needs"]):
        issues.append(issue("plan_material_need_missing", "Each material need must identify an input, or explain why external material is unnecessary.", requirement_id=requirement_id))
    for need in item["material_needs"]:
        if re.fullmatch(r"(?:无需|不需要)(?:外部)?(?:材料|资料)|no (?:external )?materials?(?: needed)?", need.strip(), re.I):
            issues.append(issue("plan_no_material_basis", "A claim of no external materials needs a reason.", requirement_id=requirement_id))
    issues.extend(check_plan_constraints(item, requirement, binding))
    return issues


async def build_plan(requirements: list[dict], model: Model, requirement_refs: dict | None = None) -> dict:
    applicable = {requirement["requirement_id"]: requirement for requirement in requirements if requirement["applicable"]}
    issues = []
    if len(applicable) != sum(requirement["applicable"] for requirement in requirements):
        issues.append(issue("duplicate_requirement_id", "Cannot plan ambiguous requirement identities."))
    if not applicable:
        return {"items": [], "issues": issues, "blocking": bool(issues)}
    bindings = {requirement_id: constraint_binding(requirement) for requirement_id, requirement in applicable.items()}
    try:
        plan = Plan.model_validate(await model.call("plan", {
            "requirements": list(applicable.values()),
            "constraint_bindings": {key: {"constraint_digest": binding["constraint_digest"]} for key, binding in bindings.items()},
        })).model_dump()
    except (ModelSemanticError, ValidationError) as exc:
        return {"items": [], "issues": [*issues, issue("plan_semantic_failure", f"Plan contract rejected ({type(exc).__name__}).")], "blocking": True}
    seen = set()
    for item in plan["items"]:
        requirement_id = item["requirement_id"]
        if requirement_id not in applicable:
            issues.append(issue("plan_unknown_requirement", "Plan refers to an absent or inapplicable requirement.", requirement_id=requirement_id))
            continue
        if requirement_id in seen:
            issues.append(issue("plan_duplicate_requirement", "Each applicable requirement must have exactly one primary response item.", requirement_id=requirement_id))
        seen.add(requirement_id)
        issues.extend(_item_issues(item, applicable[requirement_id], bindings[requirement_id]))
        reference = (requirement_refs or {}).get(requirement_id)
        if requirement_refs is not None and (
            not isinstance(reference, dict) or not reference.get("artifact_ref")
            or reference.get("requirement_id") != requirement_id
        ):
            issues.append(issue("plan_requirement_reference_missing", "Plan requires a durable reference to the exact original requirement.", requirement_id=requirement_id))
        item["requirement_ref"] = {"artifact_ref": reference.get("artifact_ref") if isinstance(reference, dict) else None,
                                   "requirement_id": requirement_id}
    for requirement_id in applicable.keys() - seen:
        issues.append(issue("plan_requirement_missing", "Applicable requirement has no concrete response item.", requirement_id=requirement_id))
    return {"items": plan["items"], "issues": issues, "blocking": bool(issues)}
