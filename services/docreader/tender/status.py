"""Host-owned lifecycle states; wire and SQL values remain plain strings."""

from enum import StrEnum

class AuthoringTask(StrEnum):
    """Model tasks whose wire values stay plain strings."""

    OUTLINE = "outline"
    DIRECTORY_PLAN = "directory_plan"
    BASIS = "authoring_basis"
    WRITE = "authoring_write"
    REVIEW = "authoring_review"
    STRUCTURE = "authoring_structure"
    ITEM_PLACEMENT = "item_placement"
    PROPOSAL_OUTLINE = "proposal_outline"


class AuthoringPrompt(StrEnum):
    """Prompt files. Assembly is a prompt variant, not a separate model task."""

    OUTLINE = AuthoringTask.OUTLINE
    ASSEMBLE = "outline_assemble"
    BID_FORMAT_OUTLINE = "bid_format_outline"
    BASIS = AuthoringTask.BASIS
    WRITE = AuthoringTask.WRITE
    REVIEW = AuthoringTask.REVIEW
    STRUCTURE = AuthoringTask.STRUCTURE
    ITEM_PLACEMENT = AuthoringTask.ITEM_PLACEMENT
    PROPOSAL_OUTLINE = AuthoringTask.PROPOSAL_OUTLINE


class DocumentRole(StrEnum):
    MAIN = "main"
    ATTACHMENT = "attachment"
    CLARIFICATION = "clarification"
    AMENDMENT = "amendment"


# Document model tasks. Structure review reuses the content-review prompt bytes.
AUTHORING_MODEL_TASKS = tuple(AuthoringTask)


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
