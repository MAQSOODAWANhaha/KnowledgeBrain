"""Only provider-reported token values contribute to cumulative usage."""

from dataclasses import dataclass, field
from typing import Any


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
