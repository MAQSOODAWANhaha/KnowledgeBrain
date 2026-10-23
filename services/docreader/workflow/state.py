"""Small checkpoint state: business documents live behind immutable file references."""
from typing import TypedDict

from docreader.tender.schema import ArtifactRef, RunStatus


class TenderState(TypedDict, total=False):
    run_id: str
    source_version: int
    domain_revision: int
    status: RunStatus
    registered_ref: ArtifactRef
    frozen_ref: ArtifactRef
    pack_plan_ref: ArtifactRef
    items_ref: ArtifactRef
    verification_ref: ArtifactRef
    checklist_digest: str


STAGES = ("register", "freeze", "extract", "verify", "confirm")
WORKFLOW_PROTOCOL = {"schema_version": 2, "normalization_version": 8, "stages": list(STAGES)}
