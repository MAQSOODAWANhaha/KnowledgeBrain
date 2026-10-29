"""Host-owned lifecycle states; wire and SQL values remain plain strings."""
from enum import StrEnum


class RunStatus(StrEnum):
    PENDING = "pending"
    RUNNING = "running"
    WAITING_CONFIRMATION = "waiting-confirmation"
    ANALYSIS_READY = "analysis-ready"
    RESPONSE_PLAN_READY = "response-plan-ready"
    BLOCKED = "blocked"
    FAILED = "failed"
    CANCELLED = "cancelled"


class CommandStatus(StrEnum):
    PENDING = "pending"
    CLAIMED = "claimed"
    PUBLISHED = "published"
    RUNNING = "running"
    COMPLETED = "completed"
    FAILED = "failed"
    CANCELLED = "cancelled"


class ModelRequestStatus(StrEnum):
    PREPARED = "prepared"
    SENT = "sent"
    SUCCEEDED = "succeeded"
    FAILED = "failed"
    UNCERTAIN = "uncertain"


class AuthoringExecutionStatus(StrEnum):
    PENDING = "pending"
    RUNNING = "running"
    SUCCEEDED = "succeeded"
    FAILED = "failed"
    CANCELLED = "cancelled"


class AuthoringJobStatus(StrEnum):
    PENDING = "pending"
    PUBLISHED = "published"
    SUCCEEDED = "succeeded"
    FAILED = "failed"
    CANCELLED = "cancelled"


class ProjectStatus(StrEnum):
    OPEN = "open"
    ENDED = "ended"


def status_row(row, status_type):
    """Validate at the storage boundary, including rows returned to callers."""
    if row is not None:
        row["status"] = status_type(row["status"])
    return row
