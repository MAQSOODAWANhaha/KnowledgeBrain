"""Small checkpoint state: business documents live behind immutable file references."""
from typing import TypedDict


class TenderState(TypedDict, total=False):
    run_id: str
    version: int
    status: str
    registered: dict
    frozen: dict
    requirements: dict
    checked: dict
    plan: dict
    checklist_digest: str


STAGES = ("register", "freeze", "extract", "verify", "confirm", "plan")
