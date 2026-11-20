"""Read-only stage report for one authoring execution. It sends nothing and persists nothing."""
from collections import Counter

from docreader.llm.usage import budget_observation

# Stage names follow business-workflow.md「编制实际模型调用」.
STAGES = {
    "directory_plan": "A 目录规划",
    "directory_material": "M 章内材料",
    "directory_requirement": "B 要求归属",
    "directory_classification": "C 归属分类",
    "directory_parameter": "P 长章展开 / 原文选择",
    "directory_response_form": "F 响应原式",
    "outline": "模型大纲（备用路径）",
    "authoring_structure": "结构复核",
    "authoring_basis": "依据",
    "authoring_write": "W 写作 / 改稿",
    "authoring_review": "正文复核 / 再核",
}


def _targets(chapters):
    for chapter in chapters:
        yield from chapter.get("targets", [])
        yield from _targets(chapter.get("children", []))


def _counts(entries):
    return dict(sorted(Counter(entry["code"] for entry in entries).items()))


def authoring_report(execution, requests, outline, document=None):
    observed = budget_observation(requests)["tasks"]
    stages = {}
    for task, summary in observed.items():
        rows = [row for row in requests if row["task"] == task]
        logical = {row["metadata"].get("logical_request_digest", row["metadata"].get("request_digest")) for row in rows}
        stages[task] = {"stage": STAGES.get(task, task), "logical_requests": len(logical),
                        **{key: summary[key] for key in ("requests", "statuses", "reported_tokens", "completion", "context")}}
    targets = list(_targets(outline.get("chapters", [])))
    report = {
        "execution_id": execution["execution_id"], "status": execution["status"],
        "kind": "outline" if document is None else "document",
        "requests": len(requests), "stages": stages,
        "targets": {"total": len(targets),
                    "knowledge_answers": sum(target["target_id"].endswith(":answer") for target in targets),
                    "with_rule_refs": sum(bool(target.get("rule_refs")) for target in targets)},
        "outline_issues": _counts(outline.get("issues", [])),
    }
    if document is not None:
        report["document_gaps"] = _counts(document.get("gaps", []))
    return report
