"""Read-only stage report for one authoring execution. It sends nothing and persists nothing."""
import asyncio
from collections import Counter

from docreader.llm.request import request_digest
from docreader.llm.usage import budget_observation, reported_usage
from docreader.tender.authoring_execution import sent_request

# Stage names follow workflow.md「编制实际模型调用」.
STAGES = {
    "directory_plan": "格式章节选择",
    "item_placement": "事项归属",
    "proposal_outline": "方案子大纲",
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


def failure_table(requests, rejections, endpoint):
    """Stage x kind x code: failed sends and host contract rejections with their tokens.

    `rejections` are model-rejection artifacts with their `request_ref` payload
    loaded as `payload`; each rejection counts once.
    """
    # A rejected response came from a send that succeeded at transport;
    # resends of the same request may share its digest after failures.
    rows = {row["metadata"].get("request_digest"): row for row in requests if row["status"] == "succeeded"}
    groups = {}

    def add(task, kind, code, row):
        group = groups.setdefault((task, kind, code), {
            "stage": STAGES.get(task, task), "task": task, "kind": kind, "code": code,
            "count": 0, "reported_tokens": 0, "unknown_usage": 0})
        group["count"] += 1
        total = (reported_usage(row["usage"]) or {}).get("total_tokens") if row else None
        if type(total) is int:
            group["reported_tokens"] += total
        else:
            group["unknown_usage"] += 1

    for row in requests:
        if row["status"] in {"failed", "uncertain"}:
            add(row["task"], "send", (row.get("error") or {}).get("type") or "unknown", row)
    for rejection in rejections:
        row = rows.get(request_digest(endpoint, sent_request(rejection["payload"]))) if endpoint else None
        errors = rejection.get("errors") or [{}]
        add(rejection["task"], "contract", errors[0].get("code") or "unknown", row)
    return sorted(groups.values(), key=lambda group: (
        -group["reported_tokens"], -group["count"], group["task"], group["kind"], group["code"]))


def authoring_report(execution, requests, outline, document=None, rejections=()):
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
        "failures": failure_table(requests, rejections, (execution.get("execution_spec") or {}).get("endpoint")),
    }
    if document is not None:
        report["document_gaps"] = _counts(document.get("gaps", []))
    return report


async def read_authoring_report(artifacts, execution):
    """Load the result and rejection artifacts one execution's report needs."""
    result = (await asyncio.to_thread(artifacts.read_json, execution["result_ref"])
              if execution.get("result_ref") else {"chapters": [], "issues": []})
    document = result if "outline_ref" in result else None
    outline = await asyncio.to_thread(artifacts.read_json, result["outline_ref"]) if document else result
    rejections = []
    for receipt in execution["receipts"]:
        if receipt["stage"] == "model-rejection":
            rejection = await asyncio.to_thread(artifacts.read_json, receipt["artifact_ref"])
            rejection["payload"] = await asyncio.to_thread(artifacts.read_json, rejection["request_ref"])
            rejections.append(rejection)
    return authoring_report(execution, execution["requests"], outline, document, rejections)
