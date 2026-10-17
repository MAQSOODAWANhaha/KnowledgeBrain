"""Only provider-reported token values contribute to cumulative usage."""

from dataclasses import dataclass, field
from collections import Counter
import math
from typing import Any


def budget_observation(requests: list[dict], plans=()) -> dict:
    """Read-only calibration evidence; unknown usage is never zero billed cost."""
    summaries = {"tasks": {}, "operations": {}}
    buckets = {}
    for row in requests:
        metadata = row.get("metadata", {})
        operation = row["task"]
        if operation == "verify":
            work_unit = metadata.get("work_unit_id", "")
            operation = next((name for name in ("discovery", "reference", "joint")
                              if work_unit.startswith(name + "-")), metadata.get("operation", "verify_unattributed"))
        for kind, name in (("tasks", row["task"]), ("operations", operation)):
            buckets.setdefault((kind, name), []).append(row)
    fields = ("prompt_tokens", "completion_tokens", "total_tokens")
    for (kind, name), rows in sorted(buckets.items()):
        usages = [reported_usage(row.get("usage")) or {} for row in rows]
        outputs = sorted(u["completion_tokens"] for u in usages if "completion_tokens" in u)
        comparable = [(u["completion_tokens"], row["metadata"]["output_reserve"])
                      for row, u in zip(rows, usages, strict=True) if "completion_tokens" in u
                      and type(row.get("metadata", {}).get("output_reserve")) is int]
        totals = [u["total_tokens"] for u in usages if "total_tokens" in u]
        capacities = [(u["total_tokens"], row["metadata"]["context_tokens"])
                      for row, u in zip(rows, usages, strict=True) if "total_tokens" in u
                      and type(row.get("metadata", {}).get("context_tokens")) is int]
        successful_outputs = sorted(u["completion_tokens"] for row, u in zip(rows, usages, strict=True)
                                    if row["status"] == "succeeded" and "completion_tokens" in u)
        summaries[kind][name] = {
            "requests": len(rows), "statuses": dict(Counter(row["status"] for row in rows)),
            "reported_tokens": {key: sum(u[key] for u in usages if key in u)
                                for key in fields if any(key in u for u in usages)},
            "missing_token_fields": {key: sum(key not in u for u in usages) for key in fields},
            "completion": {"observations": len(outputs), "max": max(outputs) if outputs else None,
                           "p95": outputs[math.ceil(len(outputs) * .95) - 1] if outputs else None,
                           "reserve_comparisons": len(comparable),
                           "exceeds_reserve": sum(actual > reserve for actual, reserve in comparable)},
            "successful_completion": {"observations": len(successful_outputs),
                "max": max(successful_outputs) if successful_outputs else None,
                "p95": successful_outputs[math.ceil(len(successful_outputs) * .95) - 1] if successful_outputs else None},
            "context": {"observations": len(totals), "max_reported_total": max(totals) if totals else None,
                        "capacity_comparisons": len(capacities),
                        "exceeds_context": sum(actual > capacity for actual, capacity in capacities)},
        }
    planned = []
    for plan in plans:
        budgets = [entry["budget"] for entry in plan["requests"]]
        planned.append({"phase": plan["phase"], "planned_requests": len(budgets),
                        "input_estimate": sum(b["input_tokens"] for b in budgets),
                        "max_input_estimate": max((b["input_tokens"] for b in budgets), default=0),
                        "output_reserve_total": sum(b["output_reserve"] for b in budgets),
                        "over_budget_requests": sum(b["fits"] is False for b in budgets)})
    unplanned = [phase for phase in ("extract", "review", "locate", "associate", "joint")
                 if phase not in {plan["phase"] for plan in planned}]
    known_requests = sum(plan["planned_requests"] for plan in planned)
    return {"calibration_status": "observations_only", "requests": len(requests), **summaries,
            "planning": {"unplanned_phases": unplanned, "known_planned_requests": known_requests,
                         "total_planned_requests": None if unplanned else known_requests},
            "plans": planned, "limits": ["Estimates and reserves are not billed usage or provider output limits.",
                "Missing usage remains unknown; failed/uncertain attempts remain included.",
                "Successful completion samples exclude failed/uncertain transfers, not semantic defects; verify output quality separately.",
                "Planned request counts exclude OCR, retries and corrections; unplanned downstream calls are unknown, not zero.",
                "Observed p95 uses nearest rank and is not a capacity guarantee, especially for small samples."]}


def reported_usage(usage: Any) -> dict | None:
    if usage is None:
        return None
    raw = usage.model_dump(exclude_none=True) if hasattr(usage, "model_dump") else usage
    if not isinstance(raw, dict):
        return None

    def counters(value):
        if isinstance(value, dict):
            return {key: clean for key, item in value.items() if (clean := counters(item)) is not None}
        return value if type(value) is int and value >= 0 else None

    result = {key: counters(raw[key]) for key in (
        "prompt_tokens", "completion_tokens", "total_tokens", "prompt_tokens_details", "completion_tokens_details",
    ) if key in raw and counters(raw[key]) is not None}
    return result or None


@dataclass
class UsageLedger:
    """Convenient process-local summary; durable request rows remain authoritative."""

    requests: int = 0
    statuses: dict[str, int] = field(default_factory=dict)
    reported_requests: int = 0
    tokens: dict[str, int] = field(default_factory=dict)

    def record(self, status: str, usage: dict | None) -> None:
        self.requests += 1
        self.statuses[status] = self.statuses.get(status, 0) + 1
        if usage is not None:
            self.reported_requests += 1
            for key in ("prompt_tokens", "completion_tokens", "total_tokens"):
                if key in usage:
                    self.tokens[key] = self.tokens.get(key, 0) + usage[key]

    def snapshot(self) -> dict:
        return {"requests": self.requests, "statuses": dict(self.statuses),
                "reported_usage_requests": self.reported_requests,
                "missing_usage_requests": self.requests - self.reported_requests,
                "reported_tokens": dict(self.tokens)}
