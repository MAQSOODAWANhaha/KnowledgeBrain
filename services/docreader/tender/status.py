"""Host-owned lifecycle states; wire and SQL values remain plain strings."""

from enum import StrEnum

class AuthoringTask(StrEnum):
    """Model tasks whose wire values stay plain strings."""

    OUTLINE = "outline"
    DIRECTORY_PLAN = "directory_plan"
    DIRECTORY_REQUIREMENT = "directory_requirement"
    DIRECTORY_MATERIAL = "directory_material"
    DIRECTORY_CLASSIFICATION = "directory_classification"
    DIRECTORY_PARAMETER = "directory_parameter"
    DIRECTORY_RESPONSE_FORM = "directory_response_form"
    BASIS = "authoring_basis"
    WRITE = "authoring_write"
    REVIEW = "authoring_review"
    STRUCTURE = "authoring_structure"
    ITEM_PLACEMENT = "item_placement"


class AuthoringPrompt(StrEnum):
    """Prompt files. Assembly is a prompt variant, not a separate model task."""

    OUTLINE = AuthoringTask.OUTLINE
    ASSEMBLE = "outline_assemble"
    DIRECTORY_PLAN = AuthoringTask.DIRECTORY_PLAN
    BID_FORMAT_OUTLINE = "bid_format_outline"
    DIRECTORY_REF_PLAN = "directory_ref_plan"
    DIRECTORY_REF_PLAN_V2 = "directory_ref_plan_v2"
    DIRECTORY_REQUIREMENT = AuthoringTask.DIRECTORY_REQUIREMENT
    DIRECTORY_MATERIAL = AuthoringTask.DIRECTORY_MATERIAL
    DIRECTORY_REQUIREMENT_NATIVE = "directory_requirement_native"
    DIRECTORY_CLASSIFICATION_NATIVE = "directory_classification_native"
    DIRECTORY_CLASSIFICATION_REVIEWED_FACTS = "directory_classification_reviewed_facts"
    DIRECTORY_CLASSIFICATION = AuthoringTask.DIRECTORY_CLASSIFICATION
    DIRECTORY_PARAMETER = AuthoringTask.DIRECTORY_PARAMETER
    DIRECTORY_NATIVE_BODY = "directory_native_body"
    DIRECTORY_MATERIAL_RESPONSE_FORMS = "directory_material_response_forms"
    DIRECTORY_MATERIAL_SOURCE_GROUPS = "directory_material_source_groups"
    DIRECTORY_MATERIAL_CLOSURE = "directory_material_closure"
    DIRECTORY_RESPONSE_FORM = AuthoringTask.DIRECTORY_RESPONSE_FORM
    DIRECTORY_RESPONSE_FORM_V2 = "directory_response_form_v2"
    DIRECTORY_RESPONSE_FORM_V3 = "directory_response_form_v3"
    BASIS = AuthoringTask.BASIS
    WRITE = AuthoringTask.WRITE
    REVIEW = AuthoringTask.REVIEW
    STRUCTURE = AuthoringTask.STRUCTURE
    ITEM_PLACEMENT = AuthoringTask.ITEM_PLACEMENT


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
